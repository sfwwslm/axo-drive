//! 内存路径锁：用于串行化冲突写操作。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time;

/// Manages asynchronous mutexes keyed by storage-relative path.
#[derive(Debug, Default)]
pub struct LockManager {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl LockManager {
    /// 创建新的锁管理器实例。
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// 在给定超时时间内获取路径锁，超时返回 Err。
    pub async fn lock_path_with_timeout(
        &self,
        path: &str,
        timeout: Duration,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, ()> {
        let key = normalize_lock_key(path);
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        time::timeout(timeout, lock.lock_owned())
            .await
            .map_err(|_| ())
    }
}

fn normalize_lock_key(path: &str) -> String {
    let trimmed = path.trim();
    let trimmed = trimmed.trim_start_matches(['/', '\\']);
    trimmed.replace('\\', "/")
}
