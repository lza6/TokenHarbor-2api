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
        let mut fallback: Option<(f64, i64, usize, &Credential)> = None;
        for (i, c) in creds.iter().enumerate() {
            if let Some(ex) = exclude {
                if c.id == ex {
                    continue;
                }
            }
            let st = states.get(&c.id).cloned().unwrap_or_default();
            if let Some(until) = st.cooling_until {
                if until > now {
                    continue; // 冷却未结束：一律跳过（防循环触发上游风控）
                }
            }
            // last_used：None(未用过) 优先；否则越旧越好
            let last = st.last_used.unwrap_or(i64::MIN);
            let score = st.health;
            if st.health >= 0.3 {
                let better = match best {
                    None => true,
                    Some((bs, blast, _, _)) => {
                        score > bs + 0.001 || ((score - bs).abs() <= 0.001 && last < blast)
                    }
                };
                if better {
                    best = Some((score, last, i, c));
                }
            } else {
                // 回退池：唯一凭证被 429 风暴打低健康分后，冷却结束仍继续服务，
                // 避免“无健康凭证”导致全网关 401 空窗（原缺陷）
                let better = match fallback {
                    None => true,
                    Some((fs, flast, _, _)) => {
                        score > fs + 0.001 || ((score - fs).abs() <= 0.001 && last < flast)
                    }
                };
                if better {
                    fallback = Some((score, last, i, c));
                }
            }
        }
        if let Some((_, _, _, c)) = best {
            return Some(c.clone());
        }
        fallback.map(|(_, _, _, c)| c.clone())
    }

    pub async fn record_success(&self, id: &str) {
        let mut states = self.states.write().await;
        let st = states.entry(id.to_string()).or_default();
        st.health = (st.health * 0.95 + 1.0).min(1.0);
        st.failures = 0;
        st.cooling_until = None;
        st.last_used = Some(chrono::Utc::now().timestamp());
    }

    /// 记录一次失败，返回冷却秒数（0=不冷却；可作 Retry-After 透传）。
    /// - 401/403：认证级失败，健康分×0.6，冷却 600s 起指数退避（上限 1h）
    /// - 429：限流非致命，健康分×0.9（轻微），冷却 30s 起指数退避（上限 3min）
    /// - 其它（502/超时）：健康分×0.95（轻微），不强制冷却
    pub async fn record_failure(&self, id: &str, status: u16) -> i64 {
        let mut states = self.states.write().await;
        let st = states.entry(id.to_string()).or_default();
        st.failures = st.failures.saturating_add(1);
        let cooldown: i64 = match status {
            401 | 403 => {
                st.health = (st.health * 0.6).max(0.0);
                (600 * st.failures.min(6) as i64).min(3600)
            }
            429 => {
                st.health = (st.health * 0.9).max(0.0);
                (30 * st.failures.min(6) as i64).min(180)
            }
            _ => {
                st.health = (st.health * 0.95).max(0.0);
                0
            }
        };
        if cooldown > 0 {
            st.cooling_until = Some(chrono::Utc::now().timestamp() + cooldown);
        }
        st.last_used = Some(chrono::Utc::now().timestamp());
        cooldown
    }

    /// 全部凭证都在冷却时的最大剩余冷却秒数（0=至少一个可用或未冷却）
    pub async fn cooling_until_max(&self) -> i64 {
        let now = chrono::Utc::now().timestamp();
        let states = self.states.read().await;
        let max = states
            .values()
            .filter_map(|s| s.cooling_until.filter(|u| *u > now).map(|u| u - now))
            .max()
            .unwrap_or(0);
        max
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pick_fallback_after_rate_limit_storm() {
        let pool = WebCookiePool::new();
        pool.add_raw("sb-auth-auth-token.0=base64-abc; th_sid=x".into())
            .await;
        let id = pool.list().await[0].id.clone();
        for _ in 0..12 {
            pool.record_failure(&id, 429).await;
        }
        let st = pool.states().await.get(&id).cloned().unwrap();
        assert!(st.health < 0.3, "health should drop below 0.3");
        assert!(pool.pick(None).await.is_none());
        {
            let mut states = pool.states.write().await;
            if let Some(s) = states.get_mut(&id) {
                s.cooling_until = Some(0);
            }
        }
        assert!(pool.pick(None).await.is_some());
    }

    #[tokio::test]
    async fn record_failure_429_is_gently_cooldown() {
        let pool = WebCookiePool::new();
        pool.add_raw("c=1".into()).await;
        let id = pool.list().await[0].id.clone();
        let cd = pool.record_failure(&id, 429).await;
        assert!(
            (30..=180).contains(&cd),
            "429 cooldown in 30..180s, got {cd}"
        );
        let cd2 = pool.record_failure(&id, 401).await;
        assert!(
            (600..=3600).contains(&cd2),
            "401 cooldown in 600..3600s, got {cd2}"
        );
    }

    #[tokio::test]
    async fn success_resets_health() {
        let pool = WebCookiePool::new();
        pool.add_raw("c=1".into()).await;
        let id = pool.list().await[0].id.clone();
        pool.record_failure(&id, 502).await;
        pool.record_success(&id).await;
        let st = pool.states().await.get(&id).cloned().unwrap();
        assert!((st.health - 1.0).abs() < 1e-9);
        assert_eq!(st.failures, 0);
        assert!(st.cooling_until.is_none());
    }
}
