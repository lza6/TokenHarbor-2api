//! TokenHarbor2API 网关入口

use std::sync::Arc;
use std::time::Duration;
use tokenharbor2api::api::{build_router, AppState};
use tokenharbor2api::config::Config;
use tokenharbor2api::models::ModelRegistry;
use tokenharbor2api::session::SessionMap;
use tokenharbor2api::upstream::UpstreamClient;
use tokenharbor2api::web_pool::WebCookiePool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,tokenharbor2api=debug")
            }),
        )
        .with_target(false)
        .init();

    let config_path = Config::resolve_config_path();
    let cfg = Config::load(config_path.as_deref())?;
    let listen_addr = cfg.listen_addr.clone();
    tracing::info!("TokenHarbor2API v{} 启动", env!("CARGO_PKG_VERSION"));
    tracing::info!("监听 {}", listen_addr);
    tracing::info!("上游 {}", cfg.upstream_base_url);
    tracing::info!("凭证数 {}", cfg.auth_tokens.len());

    // 每个凭证一个客户端（cookie 不同）；共享基础客户端用于健康检查
    let mut clients = Vec::new();
    let base_client = Arc::new(UpstreamClient::new(
        cfg.upstream_base_url.clone(),
        None,
        if cfg.http_proxy.is_empty() {
            None
        } else {
            Some(cfg.http_proxy.clone())
        },
        Duration::from_secs(cfg.request_timeout_sec),
    )?);
    let _ = base_client;

    if let Some(first) = cfg.auth_tokens.first() {
        let c = UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            Some(first.clone()),
            if cfg.http_proxy.is_empty() {
                None
            } else {
                Some(cfg.http_proxy.clone())
            },
            Duration::from_secs(cfg.request_timeout_sec),
        )?;
        clients.push(c);
    }
    // 无配置凭证时也建一个裸客户端（健康检查/匿名端点）
    if clients.is_empty() {
        let c = UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            None,
            if cfg.http_proxy.is_empty() {
                None
            } else {
                Some(cfg.http_proxy.clone())
            },
            Duration::from_secs(cfg.request_timeout_sec),
        )?;
        clients.push(c);
    }
    // 其余 config 凭证
    for t in cfg
        .auth_tokens
        .iter()
        .skip(if !cfg.auth_tokens.is_empty() { 1 } else { 0 })
    {
        if t.is_empty() {
            continue;
        }
        match UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            Some(t.clone()),
            if cfg.http_proxy.is_empty() {
                None
            } else {
                Some(cfg.http_proxy.clone())
            },
            Duration::from_secs(cfg.request_timeout_sec),
        ) {
            Ok(c) => clients.push(c),
            Err(e) => tracing::warn!("凭证客户端构造失败: {e}"),
        }
    }
    let clients = Arc::new(clients);

    // 健康检查
    if !cfg.skip_upstream_check {
        if let Some(c) = clients.first() {
            match c.check_health().await {
                Ok(_) => tracing::info!("上游健康检查通过"),
                Err(e) => tracing::warn!("上游健康检查失败（continue）: {e}"),
            }
        }
    }

    // 注册表
    let registry = Arc::new(ModelRegistry::new());
    tracing::info!("模型注册表已载入 {} 个模型", registry.all().await.len());

    // 省: registry 未实现 refresh_from_upstream —— 保留静态目录即可（TokenHarbor 无公开模型 API）

    // 凭证池
    let pool = Arc::new(WebCookiePool::new());
    pool.load(&cfg).await;
    tracing::info!(
        "凭证池: {} 条（config + tokens.json）",
        pool.list().await.len()
    );

    // 会话映射
    let sessions = Arc::new(SessionMap::new());

    // 分层并发信号量（免费/付费/多会话分桶限流）
    let semaphore = Arc::new(tokenharbor2api::semaphore::TieredSemaphore::new(
        cfg.concurrency_free_slots,
        cfg.concurrency_free_multi,
        cfg.concurrency_sub_slots,
        cfg.concurrency_sub_multi,
    ));

    // 会话自动清理任务：每 10 分钟清理空闲 >24h 的会话（防上游 200 上限风控）
    {
        let sessions2 = sessions.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
            interval.tick().await;
            loop {
                interval.tick().await;
                let stale = sessions2
                    .stale(tokenharbor2api::session::SESSION_IDLE_HOURS)
                    .await;
                for (key, binding) in &stale {
                    tracing::info!(
                        "清理空闲会话 {} (idle={}h, msgs={})",
                        &key[..key.len().min(16)],
                        tokenharbor2api::session::SESSION_IDLE_HOURS,
                        binding.message_count
                    );
                    let _ = sessions2.remove(key).await;
                }
                if !stale.is_empty() {
                    tracing::info!("会话清理完成: {} 个空闲会话已移除", stale.len());
                }
            }
        });
    }

    // 运行时 API Key
    let api_keys = Arc::new(std::sync::RwLock::new(cfg.api_keys.clone()));
    if cfg.api_keys.is_empty() {
        tracing::info!("未配置 api_keys：仅本机可访问（面板可一键生成）");
    }

    // 启动时立即续期一次（凭证过期也能自动复活）
    let proxy = if cfg.http_proxy.is_empty() {
        None
    } else {
        Some(cfg.http_proxy.clone())
    };
    {
        let pool2 = pool.clone();
        let n = pool2.refresh_creds(proxy.as_deref()).await;
        tracing::info!("启动续期完成: {n} 条凭证已刷新");
    }

    // 后台定时续期任务（每 50 分钟检查 + 刷新，覆盖 access_token 1h 有效期）
    {
        let pool2 = pool.clone();
        let proxy2 = proxy.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3000));
            interval.tick().await; // 首次立即后跳过
            loop {
                interval.tick().await;
                let n = pool2.refresh_creds(proxy2.as_deref()).await;
                if n > 0 {
                    tracing::info!("定时续期: {n} 条凭证已刷新");
                }
            }
        });
    }

    let state = AppState {
        cfg: Arc::new(cfg),
        clients,
        pool,
        registry,
        sessions,
        api_keys,
        semaphore,
        login_guard: Arc::new(tokenharbor2api::ratelimit::LoginGuard::new()),
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("HTTP 服务已启动: http://{listen_addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
