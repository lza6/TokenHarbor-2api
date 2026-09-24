//! 会话管理器：OpenAI/Anthropic 会话 ↔ 上游 TokenHarbor session
//!
//! 每个下游会话（或线程）映射一个上游 session id，实现：
//! - 多轮上下文连续（上游 messages 历史）
//! - 临时会话落库
//! - rewindTo 编辑/重生成

use crate::upstream::UpstreamClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// 会话上限（对齐上游 200 会话限制；超过时先清最旧再建，避免触发上游风控）
pub const MAX_SESSIONS: usize = 200;
/// 会话空闲清理阈值（小时）
pub const SESSION_IDLE_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    /// 上游 session id
    pub upstream_id: String,
    /// 绑定的模型（首次创建）
    pub model: String,
    pub created_at: String,
    /// 最近活动
    pub last_active: String,
    /// 消息数（用于自动滚动历史）
    pub message_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SessionMap {
    inner: Arc<RwLock<HashMap<String, SessionBinding>>>,
    /// per-key 会话创建互斥锁（防并发重复建会话触发上游风控）
    locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, key: &str) -> Option<SessionBinding> {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: &str, binding: SessionBinding) {
        self.inner.write().await.insert(key.to_string(), binding);
    }

    pub async fn remove(&self, key: &str) -> Option<SessionBinding> {
        let removed = self.remove_inner(key).await;
        // 同时清理 per-key 锁，防止锁 map 无界增长（用户可注入任意 key -> DoS）
        self.locks.lock().await.remove(key);
        removed
    }

    /// 只删绑定，不碰锁（供 ensure 持锁时内部调用，避免死锁）
    async fn remove_inner(&self, key: &str) -> Option<SessionBinding> {
        self.inner.write().await.remove(key)
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    /// 绑定（或复用）一个下游 key → 上游 session
    /// 同 key 并发时用 per-key 互斥锁：只允许一个 create_session，其余等待复用
    pub async fn ensure(
        &self,
        key: &str,
        client: &UpstreamClient,
        model: &str,
        cookie: Option<&str>,
    ) -> Result<SessionBinding> {
        // 先快速路径：已有同模型 session 直接复用（不加锁）
        if let Some(b) = self.get(key).await {
            if b.model == model {
                return Ok(b);
            }
        }
        // per-key 锁：同 key 并发建会话串行化
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        // 二次检查：等锁期间可能已被其它请求创建
        if let Some(b) = self.get(key).await {
            if b.model == model {
                return Ok(b);
            }
            let _ = self.remove_inner(key).await;
        }
        // 超上限：淘汰最旧空闲会话（防上游 200 会话风控）
        if self.len().await >= MAX_SESSIONS {
            let stale_keys = self.stale(0).await; // idle>0h 即最旧
            if let Some((old_key, _)) = stale_keys.first() {
                tracing::info!("会话池达上限 {}，淘汰最旧会话 {old_key}", MAX_SESSIONS);
                let _ = self.remove_inner(old_key).await;
            }
        }
        let upstream_id = client.create_session(model, true, cookie).await?;
        let binding = SessionBinding {
            upstream_id,
            model: model.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_active: chrono::Utc::now().to_rfc3339(),
            message_count: 0,
        };
        self.insert(key, binding.clone()).await;
        Ok(binding)
    }

    pub async fn touch(&self, key: &str, delta_messages: usize) {
        let mut map = self.inner.write().await;
        if let Some(b) = map.get_mut(key) {
            b.last_active = chrono::Utc::now().to_rfc3339();
            b.message_count += delta_messages;
        }
    }

    /// 超过 max_idle 小时未活动的会话建议清理（返回 key 列表，按最旧在前）
    /// max_idle_hours=0 时返回按 last_active 升序（最旧在前）
    pub async fn stale(&self, max_idle_hours: i64) -> Vec<(String, SessionBinding)> {
        let now = chrono::Utc::now();
        let mut items: Vec<(String, SessionBinding)> = self
            .inner
            .read()
            .await
            .iter()
            .filter(|(_, b)| {
                if max_idle_hours == 0 {
                    return true;
                }
                chrono::DateTime::parse_from_rfc3339(&b.last_active)
                    .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() > max_idle_hours)
                    .unwrap_or(false)
            })
            .map(|(k, b)| (k.clone(), b.clone()))
            .collect();
        items.sort_by(|a, b| {
            let ta = chrono::DateTime::parse_from_rfc3339(&a.1.last_active)
                .map(|t| t.timestamp())
                .unwrap_or(0);
            let tb = chrono::DateTime::parse_from_rfc3339(&b.1.last_active)
                .map(|t| t.timestamp())
                .unwrap_or(0);
            ta.cmp(&tb)
        });
        items
    }
}
