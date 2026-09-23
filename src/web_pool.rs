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
        Self { health: 1.0, failures: 0, cooling_until: None, last_used: None }
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

    /// 选一个可用凭证：健康分最高、不在冷却期
    pub async fn pick(&self, exclude: Option<&str>) -> Option<Credential> {
        let now = chrono::Utc::now().timestamp();
        let creds = self.creds.read().await;
        let states = self.states.read().await;
        let mut best: Option<(f64, usize, &Credential)> = None;
        for (i, c) in creds.iter().enumerate() {
            if let Some(ex) = exclude {
                if c.id == ex { continue; }
            }
            let st = states.get(&c.id).cloned().unwrap_or_default();
            if let Some(until) = st.cooling_until {
                if until > now { continue; }
            }
            let score = st.health - (i as f64) * 0.001;
            if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                best = Some((score, i, c));
            }
        }
        best.map(|(_, _, c)| c.clone())
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
        if status == 401 || status == 403 {
            // 冷却 10 分钟；连续失败指数退避
            let base = 600 * st.failures.min(6) as i64;
            st.cooling_until = Some(chrono::Utc::now().timestamp() + base);
        }
        st.last_used = Some(chrono::Utc::now().timestamp());
    }

    pub async fn states(&self) -> HashMap<String, CredState> {
        self.states.read().await.clone()
    }

    async fn save(&self) {
        let path = self.path.read().await.clone();
        if let Some(p) = path {
            let creds = self.creds.read().await.clone();
            let _ = std::fs::write(&p, serde_json::to_string_pretty(&creds).unwrap_or_default());
        }
    }
}

