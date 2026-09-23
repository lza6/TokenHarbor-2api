//! TokenHarbor2API 网关入口

use tokenharbor2api::api::{build_router, AppState};
use tokenharbor2api::config::Config;
use tokenharbor2api::models::ModelRegistry;
use tokenharbor2api::session::SessionMap;
use tokenharbor2api::upstream::UpstreamClient;
use tokenharbor2api::web_pool::WebCookiePool;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tokenharbor2api=debug")),
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
    let base_client = Arc::new(
        UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            None,
            if cfg.http_proxy.is_empty() { None } else { Some(cfg.http_proxy.clone()) },
            Duration::from_secs(cfg.request_timeout_sec),
        )?
    );
    let _ = base_client;

    if let Some(first) = cfg.auth_tokens.first() {
        let c = UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            Some(first.clone()),
            if cfg.http_proxy.is_empty() { None } else { Some(cfg.http_proxy.clone()) },
            Duration::from_secs(cfg.request_timeout_sec),
        )?;
        clients.push(c);
    }
    // 无配置凭证时也建一个裸客户端（健康检查/匿名端点）
    if clients.is_empty() {
        let c = UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            None,
            if cfg.http_proxy.is_empty() { None } else { Some(cfg.http_proxy.clone()) },
            Duration::from_secs(cfg.request_timeout_sec),
        )?;
        clients.push(c);
    }
    // 其余 config 凭证
    for t in cfg.auth_tokens.iter().skip(if cfg.auth_tokens.first().is_some() { 1 } else { 0 }) {
        if t.is_empty() { continue; }
        match UpstreamClient::new(
            cfg.upstream_base_url.clone(),
            Some(t.clone()),
            if cfg.http_proxy.is_empty() { None } else { Some(cfg.http_proxy.clone()) },
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
    tracing::info!("凭证池: {} 条（config + tokens.json）", pool.list().await.len());

    // 会话映射
    let sessions = Arc::new(SessionMap::new());

    // 运行时 API Key
    let api_keys = Arc::new(std::sync::RwLock::new(cfg.api_keys.clone()));
    if cfg.api_keys.is_empty() {
        tracing::info!("未配置 api_keys：仅本机可访问（面板可一键生成）");
    }

    let state = AppState {
        cfg: Arc::new(cfg),
        clients,
        pool,
        registry,
        sessions,
        api_keys,
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("HTTP 服务已启动: http://{listen_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
