//! 会话清理与上传临时目录清理的后台任务。

use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

use crate::auth::{AuthConfig, prune_expired_sessions, prune_login_attempts};
use crate::config::{SESSION_PRUNE_INTERVAL_SECS, UPLOAD_CLEAN_INTERVAL_SECS};
use crate::storage::Storage;
use crate::upload::{UploadConfig, cleanup_upload_temp};

/// 启动后台任务（会话清理与上传临时目录清理）。
pub fn spawn_background_tasks(
    storage: Arc<Storage>,
    auth: Arc<AuthConfig>,
    upload: Arc<UploadConfig>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SESSION_PRUNE_INTERVAL_SECS));
        loop {
            interval.tick().await;
            prune_expired_sessions(&auth).await;
            prune_login_attempts(&auth).await;
        }
    });

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(UPLOAD_CLEAN_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(err) = cleanup_upload_temp(&storage, &upload).await {
                warn!(error = %err, "upload temp cleanup failed");
            }
        }
    });
}
