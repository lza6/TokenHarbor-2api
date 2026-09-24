//! 分层并发信号量：免费/付费/多会话分桶限流
//!
//! 防止同一账号并发请求过多触发上游风控。
//! - free_slots: 免费模型单会话并发（默认 1）
//! - free_multi: 免费模型多会话并发（默认 3）
//! - sub_slots: 付费单会话并发（默认 3）
//! - sub_multi: 付费多会话并发（默认 8）

use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub struct TieredSemaphore {
    free_slots: Arc<Semaphore>,
    free_multi: Arc<Semaphore>,
    sub_slots: Arc<Semaphore>,
    sub_multi: Arc<Semaphore>,
}

impl TieredSemaphore {
    pub fn new(free_slots: usize, free_multi: usize, sub_slots: usize, sub_multi: usize) -> Self {
        Self {
            free_slots: Arc::new(Semaphore::new(free_slots.max(1))),
            free_multi: Arc::new(Semaphore::new(free_multi.max(1))),
            sub_slots: Arc::new(Semaphore::new(sub_slots.max(1))),
            sub_multi: Arc::new(Semaphore::new(sub_multi.max(1))),
        }
    }

    /// 获取并发许可：免费模型走 free 桶，付费走 sub 桶；多会话放宽
    pub async fn acquire(&self, is_free: bool, multi_session: bool) -> OwnedSemaphorePermit {
        if is_free {
            if multi_session {
                self.free_multi
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("semaphore closed")
            } else {
                self.free_slots
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("semaphore closed")
            }
        } else if multi_session {
            self.sub_multi
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed")
        } else {
            self.sub_slots
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed")
        }
    }
}
