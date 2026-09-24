//! 轻量登录防爆破：固定时间窗内失败次数指数退避
//! 保护 /api/ui/login、/api/tokens/login 等高价值密码端点。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct LoginGuard {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

#[derive(Clone, Debug)]
struct Entry {
    failures: u32,
    window_start: Instant,
    // 失败后该 key 锁到何时（指数退避：2^failures 秒）
    locked_until: Option<Instant>,
    // 滑动窗口：窗口内最新一次失败时间
    last_fail: Option<Instant>,
}

impl LoginGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查是否可在 key 上继续登录。锁定期间返回剩余秒数。
    pub async fn check(&self, key: &str) -> Option<u64> {
        let mut m = self.inner.lock().await;
        let now = Instant::now();
        if let Some(e) = m.get_mut(key) {
            // 30s 滑动窗口过期则重置
            if e.window_start.elapsed() > Duration::from_secs(30) {
                e.failures = 0;
                e.locked_until = None;
                e.last_fail = None;
                e.window_start = now;
            }
            if let Some(until) = e.locked_until {
                if now < until {
                    return Some((until - now).as_secs() + 1);
                }
                e.locked_until = None;
            }
        }
        None
    }

    /// 记录一次失败。返回本次新增的锁定秒数（0=仅计数）。
    pub async fn record_failure(&self, key: &str) -> u64 {
        let mut m = self.inner.lock().await;
        let now = Instant::now();
        let e = m.entry(key.to_string()).or_insert_with(|| Entry {
            failures: 0,
            window_start: now,
            locked_until: None,
            last_fail: None,
        });
        // 窗口刷新
        if e.window_start.elapsed() > Duration::from_secs(30) {
            e.failures = 0;
            e.window_start = now;
            e.locked_until = None;
        }
        e.failures = e.failures.saturating_add(1);
        e.last_fail = Some(now);
        // 退避：第1次失败1s，之后2^failures，封顶60s
        let secs = (1u64 << e.failures.min(6)) as u64;
        let lock = secs.min(60);
        e.locked_until = Some(now + Duration::from_secs(lock));
        lock
    }

    /// 登录成功后清除记录
    pub async fn clear(&self, key: &str) {
        self.inner.lock().await.remove(key);
    }

    /// 清理长时间不用的条目（防 map 膨胀）
    pub async fn sweep(&self) {
        let mut m = self.inner.lock().await;
        m.retain(|_, e| e.window_start.elapsed() < Duration::from_secs(300));
    }
}
