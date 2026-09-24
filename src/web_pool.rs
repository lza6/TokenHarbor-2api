//! 凭证池：TokenHarbor Cookie 账号池（健康分 / 熔断 / 冷却 / 轮询）
//!
//! 与 Freebuff-2API web_pool 对齐：401/403 自动冷却换号，健康分动态路由。

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub cookie: String,
    pub label: String,
    pub source: String,
    #[serde(default = "default_now")]
    pub created_at: String,
    #[serde(default)]
    pub note: String,
}

fn default_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Clone)]
pub struct CredState {
    pub health: f64,
    pub failures: u32,
    pub cooling_until: Option<i64>,
    pub last_used: Option<i64>,
}

impl Default for CredState {
    fn default() -> Self {
        Self {
            health: 1.0,
            failures: 0,
            cooling_until: None,
            last_used: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebCookiePool {
    creds: Arc<RwLock<Vec<Credential>>>,
    states: Arc<RwLock<HashMap<String, CredState>>>,
    path: Arc<RwLock<Option<String>>>,
}

impl WebCookiePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// 异步初始化：从 tokens.json 加载 + 配置 auth_tokens 导入（在 tokio 运行时内调用）
    pub async fn load(&self, cfg: &Config) {
        let tokens_path = cfg.tokens_path.clone();
        if let Ok(raw) = std::fs::read_to_string(&tokens_path) {
            if let Ok(list) = serde_json::from_str::<Vec<Credential>>(&raw) {
                let mut store = self.creds.write().await;
                *store = list;
            }
        }
        if let Some(p) = std::path::Path::new(&tokens_path).parent() {
            let _ = std::fs::create_dir_all(p);
        }
        *self.path.write().await = Some(tokens_path);
        for t in &cfg.auth_tokens {
            if !t.is_empty() {
                self.add_raw(t.clone()).await;
            }
        }
    }

    pub async fn add_raw(&self, cookie: String) -> Credential {
        let c = Credential {
            id: Uuid::new_v4().to_string(),
            cookie,
            label: "导入 Cookie".into(),
            source: "config/import".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            note: String::new(),
        };
        let duplicate = {
            let creds = self.creds.read().await;
            creds.iter().find(|x| x.cookie == c.cookie).cloned()
        };
        if let Some(existing) = duplicate {
            return existing;
        }
        self.creds.write().await.push(c.clone());
        self.save().await;
        c
    }

    pub async fn list(&self) -> Vec<Credential> {
        self.creds.read().await.clone()
    }

    pub async fn get(&self, id: &str) -> Option<Credential> {
        self.creds.read().await.iter().find(|c| c.id == id).cloned()
    }

    pub async fn delete(&self, id: &str) -> bool {
        let removed = {
            let mut creds = self.creds.write().await;
            let before = creds.len();
            creds.retain(|c| c.id != id);
            creds.len() != before
        };
        if removed {
            self.states.write().await.remove(id);
            self.save().await;
        }
        removed
    }

    /// 选一个可用凭证：健康分最高 + 最旧未使用优先（轮询均衡）
    /// - 排除冷却期/健康分过低的
    /// - 同健康分时选 last_used 最旧的（避免永远选第一个 → 多账号负载均衡）
    pub async fn pick(&self, exclude: Option<&str>) -> Option<Credential> {
        let now = chrono::Utc::now().timestamp();
        let creds = self.creds.read().await;
        let states = self.states.read().await;
        let mut best: Option<(f64, i64, usize, &Credential)> = None;
        for (i, c) in creds.iter().enumerate() {
            if let Some(ex) = exclude {
                if c.id == ex {
                    continue;
                }
            }
            let st = states.get(&c.id).cloned().unwrap_or_default();
            if let Some(until) = st.cooling_until {
                if until > now {
                    continue;
                }
            }
            // 健康分过低（<0.3）跳过（连续失败过）
            if st.health < 0.3 {
                continue;
            }
            // last_used：None(未用过) 优先；否则越旧越好
            let last = st.last_used.unwrap_or(i64::MIN);
            let score = st.health;
            let better = match best {
                None => true,
                Some((bs, blast, _, _)) => {
                    score > bs + 0.001 // 健康分优先
                        || ((score - bs).abs() <= 0.001 && last < blast) // 同分取最旧
                }
            };
            if better {
                best = Some((score, last, i, c));
            }
        }
        best.map(|(_, _, _, c)| c.clone())
    }

    pub async fn record_success(&self, id: &str) {
        let mut states = self.states.write().await;
        let st = states.entry(id.to_string()).or_default();
        st.health = (st.health * 0.95 + 1.0).min(1.0);
        st.failures = 0;
        st.cooling_until = None;
        st.last_used = Some(chrono::Utc::now().timestamp());
    }

    pub async fn record_failure(&self, id: &str, status: u16) {
        let mut states = self.states.write().await;
        let st = states.entry(id.to_string()).or_default();
        st.failures += 1;
        st.health = (st.health * 0.6).max(0.0);
        match status {
            // 401/403：认证失败，冷却 10 分钟起指数退避（上限 1h）
            401 | 403 => {
                let base = 600 * st.failures.min(6) as i64;
                st.cooling_until = Some(chrono::Utc::now().timestamp() + base);
            }
            // 429：限流，短冷却 60 秒起（防连续选中触发上游风控）
            429 => {
                let base = 60 * st.failures.min(6) as i64;
                st.cooling_until = Some(chrono::Utc::now().timestamp() + base);
            }
            _ => {}
        }
        st.last_used = Some(chrono::Utc::now().timestamp());
    }

    pub async fn states(&self) -> HashMap<String, CredState> {
        self.states.read().await.clone()
    }

    /// 刷新凭证（Supabase refresh_token 换新，自动续期核心）
    pub async fn refresh_creds(&self, proxy: Option<&str>) -> usize {
        let creds = self.list().await;
        let mut refreshed = 0usize;
        for cred in &creds {
            let Some(rt) = crate::refresh::refresh_token_from_cookie(&cred.cookie) else {
                continue;
            };
            match crate::refresh::refresh_session(&rt, proxy).await {
                Ok(sess) => {
                    let new_cookie = crate::refresh::rebuild_cookie(&cred.cookie, &sess);
                    let mut store = self.creds.write().await;
                    if let Some(c) = store.iter_mut().find(|c| c.id == cred.id) {
                        c.cookie = new_cookie;
                    }
                    drop(store);
                    self.record_success(&cred.id).await;
                    self.save().await;
                    refreshed += 1;
                    tracing::info!("凭证 {} 已自动续期", &cred.id[..8]);
                }
                Err(e) => {
                    // 续期失败不惩罚凭证：access_token 可能仍有效（浏览器会话），
                    // 仅 refresh_token 链失效（refresh_token_already_used 等）
                    tracing::warn!(
                        "凭证 {} 续期失败（不影响现有 access_token 使用）: {e}",
                        &cred.id[..8]
                    );
                }
            }
        }
        refreshed
    }

    /// 邮箱密码登录并入库
    pub async fn login_email(
        &self,
        email: &str,
        password: &str,
        proxy: Option<&str>,
    ) -> anyhow::Result<Credential> {
        let sess = crate::refresh::email_login(email, password, proxy).await?;
        let cookie = format!(
            "sb-auth-auth-token.0={}; sb-auth-auth-token.1={}; th_sid={}",
            sess_token(&sess, 0),
            sess_token(&sess, 1),
            uuid::Uuid::new_v4().simple()
        );
        let cred = self.add_raw(cookie).await;
        Ok(cred)
    }

    async fn save(&self) {
        let path = self.path.read().await.clone();
        if let Some(p) = path {
            let creds = self.creds.read().await.clone();
            let _ = std::fs::write(&p, serde_json::to_string_pretty(&creds).unwrap_or_default());
        }
    }
}

/// 把 RefreshResponse 组装成 Supabase base64-<json> cookie 值
fn sess_token(sess: &crate::refresh::RefreshResponse, which: u8) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let obj = if which == 0 {
        serde_json::json!({
            "access_token": sess.access_token,
            "token_type": sess.token_type.clone().unwrap_or_else(|| "bearer".into()),
            "expires_in": sess.expires_in.unwrap_or(3600),
            "expires_at": sess.expires_at.unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600),
            "refresh_token": sess.refresh_token,
            "user": sess.user.clone().unwrap_or(serde_json::json!({})),
        })
    } else {
        serde_json::json!({
            "access_token": sess.access_token,
            "refresh_token": sess.refresh_token,
            "expires_in": sess.expires_in.unwrap_or(3600),
            "expires_at": sess.expires_at.unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600),
            "token_type": sess.token_type.clone().unwrap_or_else(|| "bearer".into()),
        })
    };
    format!(
        "base64-{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&obj).unwrap_or_default())
    )
}
