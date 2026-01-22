//! 存储层工具：在指定根目录下执行文件操作。
//!
//! 存储层负责规范化用户路径、阻止符号链接穿透，并提供
//! 列表、创建、删除等基础能力。

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::cmp::Ordering;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio::fs;
use tokio::io::ErrorKind;

use crate::etag::etag_from_metadata;
/// Filesystem-backed storage rooted at a dedicated directory.
#[derive(Clone, Debug)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// 创建以指定目录为根的存储实例。
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 确保根目录在磁盘上存在。
    pub async fn ensure_root(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root).await
    }

    /// 返回存储根目录的绝对路径。
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// 解析并校验相对路径，可选择允许末端不存在。
    pub async fn resolve_path_checked(
        &self,
        relative: &str,
        allow_missing_leaf: bool,
    ) -> Result<PathBuf, StorageError> {
        let target = self.resolve(Some(relative))?;
        self.ensure_no_symlink_components(&target, allow_missing_leaf)
            .await?;
        Ok(target)
    }

    /// 解析并校验存储根路径。
    pub async fn resolve_root_checked(&self) -> Result<PathBuf, StorageError> {
        let target = self.resolve(None)?;
        self.ensure_no_symlink_components(&target, false).await?;
        Ok(target)
    }

    fn resolve(&self, relative: Option<&str>) -> Result<PathBuf, StorageError> {
        let mut normalized = PathBuf::new();

        if let Some(value) = relative {
            let trimmed = value.trim_start_matches(['/', '\\']);
            for component in Path::new(trimmed).components() {
                match component {
                    Component::Normal(segment) => normalized.push(segment),
                    Component::CurDir => continue,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                        return Err(StorageError::InvalidPath);
                    }
                }
            }
        }

        Ok(self.root.join(normalized))
    }

    async fn ensure_no_symlink_components(
        &self,
        target: &Path,
        allow_missing_leaf: bool,
    ) -> Result<(), StorageError> {
        let relative = target
            .strip_prefix(&self.root)
            .map_err(|_| StorageError::InvalidPath)?;
        let mut current = PathBuf::from(&self.root);
        let mut components = relative.components().peekable();

        while let Some(component) = components.next() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current).await {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        return Err(StorageError::InvalidPath);
                    }
                    if components.peek().is_some() && !metadata.is_dir() {
                        return Err(StorageError::InvalidPath);
                    }
                }
                Err(err) if err.kind() == ErrorKind::NotFound && allow_missing_leaf => {
                    return Ok(());
                }
                Err(err) => return Err(StorageError::Io(err)),
            }
        }

        Ok(())
    }

    /// 列出目录内容并返回排序后的元数据。
    pub async fn list_dir(&self, relative: Option<&str>) -> Result<Vec<FileEntry>, StorageError> {
        let target = match relative {
            Some(path) => self.resolve_path_checked(path, false).await?,
            None => self.resolve_root_checked().await?,
        };
        let mut dir = fs::read_dir(&target).await?;
        let mut entries = Vec::new();

        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".upload_temp" {
                continue;
            }
            let relative_path = path
                .strip_prefix(&self.root)
                .map_err(|_| StorageError::InvalidPath)?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let modified = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
                .map(format_timestamp);

            let is_dir = metadata.is_dir();
            let etag = if is_dir {
                None
            } else {
                Some(etag_from_metadata(&metadata))
            };
            entries.push(FileEntry {
                name,
                path: relative_path,
                is_dir,
                size: metadata.len(),
                modified,
                etag,
            });
        }

        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(entries)
    }

    /// 删除存储根目录下的文件或目录（递归）。
    pub async fn delete_path(&self, relative: &str) -> Result<(), StorageError> {
        let target = self.resolve_path_checked(relative, false).await?;
        let metadata = fs::metadata(&target).await?;
        if metadata.is_dir() {
            fs::remove_dir_all(target).await?;
        } else {
            fs::remove_file(target).await?;
        }
        Ok(())
    }

    /// 在存储根目录下创建目录及其缺失的父级。
    pub async fn create_dir(&self, relative: &str) -> Result<(), StorageError> {
        let target = self.resolve_path_checked(relative, true).await?;
        fs::create_dir_all(target).await?;
        Ok(())
    }
}

fn format_timestamp(duration: Duration) -> String {
    let timestamp = UNIX_EPOCH + duration;
    let datetime: DateTime<Utc> = timestamp.into();
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 存储操作可能返回的错误类型。
#[derive(Debug)]
pub enum StorageError {
    /// 路径非法或试图越界存储根目录。
    InvalidPath,
    /// 文件系统 I/O 错误。
    Io(io::Error),
}

impl From<io::Error> for StorageError {
    fn from(err: io::Error) -> Self {
        StorageError::Io(err)
    }
}

/// `list_dir` 返回的文件或目录元数据。
#[derive(Serialize)]
pub struct FileEntry {
    /// 文件或目录名称。
    pub name: String,
    /// 存储相对路径（使用 `/` 分隔）。
    pub path: String,
    /// 是否为目录。
    pub is_dir: bool,
    /// 文件大小（字节），目录为 0。
    pub size: u64,
    /// 格式化后的修改时间。
    pub modified: Option<String>,
    /// 文件的 ETag（目录为 None）。
    pub etag: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{Storage, StorageError};
    use tempfile::tempdir;

    #[cfg(unix)]
    #[tokio::test]
    async fn resolve_path_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");

        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, b"secret").expect("write outside file");
        let link_path = root.join("link");
        symlink(&outside, &link_path).expect("symlink");

        let storage = Storage::new(root);
        let result = storage.resolve_path_checked("link", false).await;
        assert!(matches!(result, Err(StorageError::InvalidPath)));
    }
}
