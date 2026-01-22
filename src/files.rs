//! 文件列表、下载、写入与目录操作处理器。

use axum::Error as AxumError;
use axum::body::Body as AxumBody;
use axum::extract::{Extension, Json, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json as JsonResponse, Response};
use futures_util::stream::StreamExt;
use http_body_util::BodyExt;
use httpdate::{fmt_http_date, parse_http_date};
use serde::Deserialize;
use std::io::{ErrorKind, SeekFrom};
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use tracing::{debug, info};

use crate::atomic::AtomicFile;
use crate::config::DEFAULT_LOCK_WAIT_TIMEOUT_SECS;
use crate::error::ApiError;
use crate::etag::{check_preconditions, etag_from_metadata};
use crate::locking::LockManager;
use crate::storage::{FileEntry, Storage};

#[derive(Deserialize)]
pub(crate) struct OptionalPathQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RequiredPathQuery {
    path: String,
}

#[derive(Deserialize)]
pub(crate) struct DirCreateBody {
    path: String,
}

/// 列出目录内容。
pub async fn list_files(
    Query(query): Query<OptionalPathQuery>,
    Extension(storage): Extension<Arc<Storage>>,
) -> Result<JsonResponse<Vec<FileEntry>>, ApiError> {
    let entries = storage.list_dir(query.path.as_deref()).await?;
    info!(
        path = query.path.as_deref().unwrap_or(""),
        count = entries.len(),
        "list files"
    );
    Ok(JsonResponse(entries))
}

/// 下载文件，支持 Range 请求与缓存相关头。
pub async fn download_file(
    Query(RequiredPathQuery { path }): Query<RequiredPathQuery>,
    request_headers: HeaderMap,
    Extension(storage): Extension<Arc<Storage>>,
) -> Result<Response, ApiError> {
    let target = storage.resolve_path_checked(&path, false).await?;
    let metadata = fs::metadata(&target)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    if metadata.is_dir() {
        return Err(ApiError::BadRequest("path is not a file".into()));
    }
    let file_size = metadata.len();
    let modified = metadata.modified().ok();
    let last_modified = modified.map(fmt_http_date);
    let mime = mime_guess::from_path(&path).first_or_octet_stream();

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.essence_str())
            .map_err(|_| ApiError::Internal("无效的 MIME 类型".into()))?,
    );
    response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(value) = last_modified.as_deref() {
        response_headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(value)
                .map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
        );
    }
    let etag = etag_from_metadata(&metadata);
    response_headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
    );

    let if_range_matches = match request_headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => match parse_http_date(value) {
            Ok(date) => modified.map(|ts| ts <= date).unwrap_or(false),
            Err(_) => false,
        },
        None => true,
    };

    let range = if if_range_matches {
        parse_range(request_headers.get(header::RANGE), file_size)?
    } else {
        None
    };

    let file = File::open(&target)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    if let Some((start, end)) = range {
        let length = end - start + 1;
        debug!(path, start, end, length, "download range request accepted");
        let mut file = file;
        file.seek(SeekFrom::Start(start))
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let stream = ReaderStream::new(file.take(length));
        response_headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end, file_size))
                .map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
        );
        response_headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string())
                .map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
        );
        return Ok((
            StatusCode::PARTIAL_CONTENT,
            response_headers,
            AxumBody::from_stream(stream),
        )
            .into_response());
    }

    response_headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&file_size.to_string())
            .map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
    );
    info!(path, size = file_size, "download full file");
    let stream = ReaderStream::new(file);
    Ok((
        StatusCode::OK,
        response_headers,
        AxumBody::from_stream(stream),
    )
        .into_response())
}

/// 写入文件内容，支持条件写入与原子替换。
pub async fn write_file(
    Query(RequiredPathQuery { path }): Query<RequiredPathQuery>,
    headers: HeaderMap,
    Extension(storage): Extension<Arc<Storage>>,
    Extension(lock_manager): Extension<Arc<LockManager>>,
    body: AxumBody,
) -> Result<Response, ApiError> {
    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }
    info!(path, "write file");

    let _guard = lock_manager
        .lock_path_with_timeout(
            &path,
            std::time::Duration::from_secs(DEFAULT_LOCK_WAIT_TIMEOUT_SECS),
        )
        .await
        .map_err(|_| ApiError::Conflict("path locked".into()))?;
    let target = storage.resolve_path_checked(&path, true).await?;
    let metadata = match fs::metadata(&target).await {
        Ok(metadata) => Some(metadata),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => return Err(ApiError::Internal(err.to_string())),
    };
    let exists = metadata.is_some();
    let etag = metadata.as_ref().map(etag_from_metadata);
    check_preconditions(&headers, etag.as_deref(), exists)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    }

    let mut atomic = AtomicFile::new(&target).await?;
    let write_result: Result<(), ApiError> = async {
        let mut data_stream = BodyExt::into_data_stream(body);
        while let Some(chunk) = data_stream.next().await {
            let chunk = chunk.map_err(|err: AxumError| ApiError::Internal(err.to_string()))?;
            if !chunk.is_empty() {
                atomic
                    .file_mut()
                    .write_all(&chunk)
                    .await
                    .map_err(|err| ApiError::Internal(err.to_string()))?;
            }
        }
        Ok(())
    }
    .await;
    if let Err(err) = write_result {
        atomic.cleanup().await;
        return Err(err);
    }
    atomic.finalize().await?;

    let metadata = fs::metadata(&target)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    let mut response_headers = HeaderMap::new();
    let etag = etag_from_metadata(&metadata);
    response_headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
    );
    if let Ok(modified) = metadata.modified() {
        let value = fmt_http_date(modified);
        response_headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(&value)
                .map_err(|_| ApiError::Internal("响应头构建失败".into()))?,
        );
    }
    Ok((StatusCode::CREATED, response_headers).into_response())
}

/// 删除文件或目录。
pub async fn delete_entry(
    Query(RequiredPathQuery { path }): Query<RequiredPathQuery>,
    Extension(storage): Extension<Arc<Storage>>,
    Extension(lock_manager): Extension<Arc<LockManager>>,
) -> Result<StatusCode, ApiError> {
    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }
    let _guard = lock_manager
        .lock_path_with_timeout(
            &path,
            std::time::Duration::from_secs(DEFAULT_LOCK_WAIT_TIMEOUT_SECS),
        )
        .await
        .map_err(|_| ApiError::Conflict("path locked".into()))?;
    storage.delete_path(&path).await?;
    info!(path, "delete entry");
    Ok(StatusCode::NO_CONTENT)
}

/// 创建目录（含父级）。
pub async fn create_directory(
    Extension(storage): Extension<Arc<Storage>>,
    Extension(lock_manager): Extension<Arc<LockManager>>,
    payload: Json<DirCreateBody>,
) -> Result<StatusCode, ApiError> {
    let DirCreateBody { path } = payload.0;

    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }

    let _guard = lock_manager
        .lock_path_with_timeout(
            &path,
            std::time::Duration::from_secs(DEFAULT_LOCK_WAIT_TIMEOUT_SECS),
        )
        .await
        .map_err(|_| ApiError::Conflict("path locked".into()))?;
    storage.create_dir(&path).await?;
    info!(path, "create directory");
    Ok(StatusCode::CREATED)
}

/// 解析 Range 头，返回可读取的范围。
fn parse_range(
    value: Option<&HeaderValue>,
    file_size: u64,
) -> Result<Option<(u64, u64)>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if file_size == 0 {
        return Err(ApiError::RangeNotSatisfiable(file_size));
    }
    let value = value
        .to_str()
        .map_err(|_| ApiError::BadRequest("invalid Range header".into()))?;
    let Some(range) = value.strip_prefix("bytes=") else {
        return Err(ApiError::BadRequest("invalid Range header".into()));
    };
    if range.contains(',') {
        return Err(ApiError::BadRequest("multiple ranges not supported".into()));
    }

    let mut parts = range.splitn(2, '-');
    let start_part = parts.next().unwrap_or_default();
    let end_part = parts.next().unwrap_or_default();

    let (start, end) = if start_part.is_empty() {
        let suffix: u64 = end_part
            .parse()
            .map_err(|_| ApiError::BadRequest("invalid Range header".into()))?;
        if suffix == 0 {
            return Ok(None);
        }
        let start = file_size.saturating_sub(suffix);
        (start, file_size.saturating_sub(1))
    } else {
        let start: u64 = start_part
            .parse()
            .map_err(|_| ApiError::BadRequest("invalid Range header".into()))?;
        let end: u64 = if end_part.is_empty() {
            file_size.saturating_sub(1)
        } else {
            end_part
                .parse()
                .map_err(|_| ApiError::BadRequest("invalid Range header".into()))?
        };
        (start, end)
    };

    if start > end || start >= file_size || end >= file_size {
        return Err(ApiError::RangeNotSatisfiable(file_size));
    }

    Ok(Some((start, end.min(file_size.saturating_sub(1)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Query;
    use axum::http::HeaderMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    use crate::locking::LockManager;

    fn make_storage() -> (tempfile::TempDir, Arc<Storage>) {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("storage");
        std::fs::create_dir_all(&root).expect("create storage root");
        (temp, Arc::new(Storage::new(root)))
    }

    #[tokio::test]
    async fn write_file_rejects_traversal_path() {
        let (_temp, storage) = make_storage();
        let lock_manager = Arc::new(LockManager::new());
        let result = write_file(
            Query(RequiredPathQuery {
                path: "../secret.txt".to_string(),
            }),
            HeaderMap::new(),
            Extension(storage),
            Extension(lock_manager),
            AxumBody::from("data"),
        )
        .await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }
}
