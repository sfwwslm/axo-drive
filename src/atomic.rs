//! 临时写入与原子替换的辅助方法。

use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use uuid::Uuid;

use crate::error::ApiError;

/// 可用于原子替换的临时文件封装。
pub struct AtomicFile {
    target: PathBuf,
    temp_path: PathBuf,
    file: File,
}

impl AtomicFile {
    /// 在目标路径同目录创建临时文件。
    pub async fn new(target: &Path) -> Result<Self, ApiError> {
        let parent = target
            .parent()
            .ok_or_else(|| ApiError::BadRequest("invalid target path".into()))?;
        let base = target
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "file".into());
        let temp_name = format!(".{base}.tmp.{}", Uuid::new_v4());
        let temp_path = parent.join(temp_name);
        let file = File::create(&temp_path)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        Ok(Self {
            target: target.to_path_buf(),
            temp_path,
            file,
        })
    }

    /// 返回临时文件的可写句柄。
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// 放弃并清理临时文件。
    pub async fn cleanup(self) {
        let _ = fs::remove_file(&self.temp_path).await;
    }

    /// 同步并原子替换目标文件。
    pub async fn finalize(self) -> Result<(), ApiError> {
        self.file
            .sync_all()
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        drop(self.file);

        if let Some(parent) = self.target.parent() {
            let _ = sync_dir(parent).await;
        }

        if let Err(err) = fs::rename(&self.temp_path, &self.target).await {
            #[cfg(windows)]
            {
                if fs::remove_file(&self.target).await.is_ok() {
                    fs::rename(&self.temp_path, &self.target)
                        .await
                        .map_err(|err| ApiError::Internal(err.to_string()))?;
                } else {
                    let _ = fs::remove_file(&self.temp_path).await;
                    return Err(ApiError::Internal(err.to_string()));
                }
            }
            #[cfg(not(windows))]
            {
                let _ = fs::remove_file(&self.temp_path).await;
                return Err(ApiError::Internal(err.to_string()));
            }
        }

        if let Some(parent) = self.target.parent() {
            let _ = sync_dir(parent).await;
        }

        Ok(())
    }
}

async fn sync_dir(path: &Path) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let dir = std::fs::File::open(path)?;
        dir.sync_all()
    })
    .await
    .map_err(|err| io::Error::other(err.to_string()))?
}
