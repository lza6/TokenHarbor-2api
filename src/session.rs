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
use tokio::sync::RwLock;

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
        self.inner.write().await.remove(key)
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// 绑定（或复用）一个下游 key → 上游 session
    pub async fn ensure(
        &self,
        key: &str,
        client: &UpstreamClient,
        model: &str,
        cookie: Option<&str>,
    ) -> Result<SessionBinding> {
        if let Some(b) = self.get(key).await {
            // 模型变了就重建（上游 session.model 固定）
            if b.model == model {
                return Ok(b);
            }
            let _ = self.remove(key).await;
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

    /// 超过 max_idle 小时未活动的会话建议清理（返回 key 列表）
    pub async fn stale(&self, max_idle_hours: i64) -> Vec<(String, SessionBinding)> {
        let now = chrono::Utc::now();
        self.inner.read().await.iter()
            .filter(|(_, b)| {
                chrono::DateTime::parse_from_rfc3339(&b.last_active)
                    .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() > max_idle_hours)
                    .unwrap_or(false)
            })
            .map(|(k, b)| (k.clone(), b.clone()))
            .collect()
    }
}


