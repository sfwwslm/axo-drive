//! 分片上传处理器与临时目录管理。

use axum::Error as AxumError;
use axum::body::Body as AxumBody;
use axum::extract::{Extension, Json, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json as JsonResponse, Response};
use futures_util::stream::StreamExt;
use http_body_util::BodyExt;
use httpdate::fmt_http_date;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::atomic::AtomicFile;
use crate::config::{DEFAULT_LOCK_WAIT_TIMEOUT_SECS, MAX_CHUNK_SIZE, UPLOAD_TEMP_DIR};
use crate::error::ApiError;
use crate::etag::{check_preconditions, etag_from_metadata};
use crate::locking::LockManager;
use crate::storage::Storage;

#[derive(Debug)]
pub struct UploadConfig {
    pub max_total_size: u64,
    pub max_chunks: u64,
    pub max_concurrent: u64,
    pub temp_ttl: std::time::Duration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadInitRequest {
    name: String,
    total_size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadInitResponse {
    upload_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadMetadata {
    name: String,
    total_size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadChunkQuery {
    upload_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadCompleteRequest {
    upload_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadAbortRequest {
    upload_id: String,
}

/// 初始化上传会话，写入元数据。
pub async fn init_upload(
    Extension(storage): Extension<Arc<Storage>>,
    Extension(upload): Extension<Arc<UploadConfig>>,
    Json(payload): Json<UploadInitRequest>,
) -> Result<JsonResponse<UploadInitResponse>, ApiError> {
    let normalized_name = payload
        .name
        .trim()
        .trim_start_matches(['/', '\\'])
        .to_string();
    if normalized_name.is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    storage.resolve_path_checked(&normalized_name, true).await?;
    if upload.max_total_size > 0 && payload.total_size > upload.max_total_size {
        return Err(ApiError::BadRequest("upload size exceeds limit".into()));
    }
    if payload.total_size > 0 && upload.max_chunks > 0 {
        let expected_chunks = payload.total_size.div_ceil(MAX_CHUNK_SIZE);
        if expected_chunks > upload.max_chunks {
            return Err(ApiError::BadRequest(
                "upload chunk count exceeds limit".into(),
            ));
        }
    }
    if upload.max_concurrent > 0 {
        let active = count_upload_temp_dirs(&storage).await?;
        if active >= upload.max_concurrent {
            return Err(ApiError::TooManyRequests(60));
        }
    }

    let upload_id = Uuid::new_v4().to_string();
    let temp_dir = upload_temp_root(&storage).join(&upload_id);
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    info!(
        upload_id,
        name = normalized_name,
        total_size = payload.total_size,
        "init upload"
    );

    let metadata = UploadMetadata {
        name: normalized_name,
        total_size: payload.total_size,
    };
    let meta_path = temp_dir.join("meta.json");
    let meta_content =
        serde_json::to_vec(&metadata).map_err(|err| ApiError::Internal(err.to_string()))?;
    fs::write(meta_path, meta_content)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(JsonResponse(UploadInitResponse { upload_id }))
}

/// 上传单个分片。
pub async fn upload_chunk(
    Query(UploadChunkQuery { upload_id }): Query<UploadChunkQuery>,
    headers: HeaderMap,
    Extension(storage): Extension<Arc<Storage>>,
    Extension(upload): Extension<Arc<UploadConfig>>,
    body: AxumBody,
) -> Result<StatusCode, ApiError> {
    if upload_id.is_empty() {
        return Err(ApiError::BadRequest("upload_id is required".into()));
    }
    if Uuid::parse_str(&upload_id).is_err() {
        return Err(ApiError::BadRequest("upload_id is invalid".into()));
    }

    let chunk_index = headers
        .get("X-Chunk-Index")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| ApiError::BadRequest("X-Chunk-Index is required".into()))?;

    let temp_dir = upload_temp_root(&storage).join(&upload_id);
    let meta_path = temp_dir.join("meta.json");
    let meta_bytes = fs::read(&meta_path)
        .await
        .map_err(|_| ApiError::NotFound("upload_id not found".into()))?;
    let metadata: UploadMetadata =
        serde_json::from_slice(&meta_bytes).map_err(|err| ApiError::Internal(err.to_string()))?;
    if upload.max_total_size > 0 && metadata.total_size > upload.max_total_size {
        return Err(ApiError::BadRequest("upload size exceeds limit".into()));
    }
    if upload.max_chunks > 0 {
        let max_index = upload.max_chunks.saturating_sub(1);
        if chunk_index > max_index {
            return Err(ApiError::BadRequest("chunk index exceeds limit".into()));
        }
    }

    let chunk_path = temp_dir.join(format!("{chunk_index}.part"));
    let mut file = File::create(&chunk_path)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    let mut data_stream = BodyExt::into_data_stream(body);
    let mut total_written: u64 = 0;
    while let Some(chunk) = data_stream.next().await {
        let chunk = chunk.map_err(|err: AxumError| ApiError::Internal(err.to_string()))?;
        if chunk.is_empty() {
            continue;
        }
        total_written += chunk.len() as u64;
        if total_written > MAX_CHUNK_SIZE {
            let _ = fs::remove_file(&chunk_path).await;
            return Err(ApiError::BadRequest("chunk too large".into()));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    }

    debug!(
        upload_id,
        chunk_index,
        bytes = total_written,
        "upload chunk saved"
    );
    Ok(StatusCode::CREATED)
}

/// 合并分片并原子替换目标文件。
pub async fn complete_upload(
    headers: HeaderMap,
    Extension(storage): Extension<Arc<Storage>>,
    Extension(lock_manager): Extension<Arc<LockManager>>,
    Extension(upload): Extension<Arc<UploadConfig>>,
    Json(payload): Json<UploadCompleteRequest>,
) -> Result<Response, ApiError> {
    if payload.upload_id.trim().is_empty() {
        return Err(ApiError::BadRequest("upload_id is required".into()));
    }
    if Uuid::parse_str(&payload.upload_id).is_err() {
        return Err(ApiError::BadRequest("upload_id is invalid".into()));
    }

    let temp_dir = upload_temp_root(&storage).join(&payload.upload_id);
    let meta_path = temp_dir.join("meta.json");
    let meta_bytes = fs::read(&meta_path)
        .await
        .map_err(|_| ApiError::NotFound("upload_id not found".into()))?;
    let metadata: UploadMetadata =
        serde_json::from_slice(&meta_bytes).map_err(|err| ApiError::Internal(err.to_string()))?;

    if metadata.name.trim().is_empty() {
        return Err(ApiError::BadRequest("target name is required".into()));
    }
    if upload.max_total_size > 0 && metadata.total_size > upload.max_total_size {
        return Err(ApiError::BadRequest("upload size exceeds limit".into()));
    }

    let mut dir = fs::read_dir(&temp_dir)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    let mut parts = Vec::new();
    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.ends_with(".part") {
            continue;
        }
        let index_str = file_name.trim_end_matches(".part");
        if let Ok(index) = index_str.parse::<u64>() {
            parts.push((index, entry.path()));
        }
    }

    if parts.is_empty() {
        return Err(ApiError::BadRequest("no chunks uploaded".into()));
    }
    if upload.max_chunks > 0 && parts.len() as u64 > upload.max_chunks {
        return Err(ApiError::BadRequest(
            "upload chunk count exceeds limit".into(),
        ));
    }
    parts.sort_by_key(|(index, _)| *index);

    for (expected_index, (index, _)) in parts.iter().enumerate() {
        let expected_index = expected_index as u64;
        if *index != expected_index {
            warn!(
                upload_id = payload.upload_id,
                expected = expected_index,
                got = *index,
                "missing chunk"
            );
            return Err(ApiError::BadRequest("missing chunk".into()));
        }
    }

    let _guard = lock_manager
        .lock_path_with_timeout(
            &metadata.name,
            std::time::Duration::from_secs(DEFAULT_LOCK_WAIT_TIMEOUT_SECS),
        )
        .await
        .map_err(|_| ApiError::Conflict("path locked".into()))?;
    let target = storage.resolve_path_checked(&metadata.name, true).await?;
    let existing = match fs::metadata(&target).await {
        Ok(metadata) => Some(metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(ApiError::Internal(err.to_string())),
    };
    let exists = existing.is_some();
    let etag = existing.as_ref().map(etag_from_metadata);
    check_preconditions(&headers, etag.as_deref(), exists)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    }

    let mut atomic = AtomicFile::new(&target).await?;
    let write_result: Result<u64, ApiError> = async {
        let mut total_written: u64 = 0;
        for (_, path) in &parts {
            let mut part_file = File::open(path)
                .await
                .map_err(|err| ApiError::Internal(err.to_string()))?;
            let copied = tokio::io::copy(&mut part_file, atomic.file_mut())
                .await
                .map_err(|err| ApiError::Internal(err.to_string()))?;
            total_written += copied;
        }
        Ok(total_written)
    }
    .await;
    let total_written = match write_result {
        Ok(value) => value,
        Err(err) => {
            atomic.cleanup().await;
            return Err(err);
        }
    };

    if metadata.total_size > 0 && total_written != metadata.total_size {
        warn!(
            upload_id = payload.upload_id,
            expected = metadata.total_size,
            actual = total_written,
            "size mismatch after merge"
        );
        atomic.cleanup().await;
        return Err(ApiError::BadRequest("size mismatch".into()));
    }
    atomic.finalize().await?;

    fs::remove_dir_all(&temp_dir)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    info!(
        upload_id = payload.upload_id,
        name = metadata.name,
        total_size = metadata.total_size,
        "upload complete"
    );
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

/// 中止上传并清理临时目录。
pub async fn abort_upload(
    Extension(storage): Extension<Arc<Storage>>,
    Json(payload): Json<UploadAbortRequest>,
) -> Result<StatusCode, ApiError> {
    if payload.upload_id.trim().is_empty() {
        return Err(ApiError::BadRequest("upload_id is required".into()));
    }
    if Uuid::parse_str(&payload.upload_id).is_err() {
        return Err(ApiError::BadRequest("upload_id is invalid".into()));
    }

    let temp_dir = upload_temp_root(&storage).join(&payload.upload_id);
    if fs::metadata(&temp_dir).await.is_err() {
        return Err(ApiError::NotFound("upload_id not found".into()));
    }
    fs::remove_dir_all(&temp_dir)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    info!(upload_id = payload.upload_id, "upload aborted");
    Ok(StatusCode::NO_CONTENT)
}

/// 返回上传临时目录的根路径。
pub fn upload_temp_root(storage: &Storage) -> PathBuf {
    let temp_path = Path::new(UPLOAD_TEMP_DIR);
    if temp_path.is_absolute() {
        return temp_path.to_path_buf();
    }

    let Some(parent) = storage.root_path().parent() else {
        return PathBuf::from(UPLOAD_TEMP_DIR);
    };

    if temp_path.iter().next() == Some(OsStr::new(".axo"))
        && parent.file_name() == Some(OsStr::new(".axo"))
    {
        let rest: PathBuf = temp_path.iter().skip(1).collect();
        return if rest.as_os_str().is_empty() {
            parent.to_path_buf()
        } else {
            parent.join(rest)
        };
    }

    parent.join(temp_path)
}

/// 统计当前活跃的上传临时目录数量。
pub async fn count_upload_temp_dirs(storage: &Storage) -> Result<u64, ApiError> {
    let temp_root = upload_temp_root(storage);
    if fs::metadata(&temp_root).await.is_err() {
        return Ok(0);
    }
    let mut dir = fs::read_dir(&temp_root)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    let mut count = 0;
    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    {
        let metadata = entry
            .metadata()
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        if metadata.is_dir() {
            count += 1;
        }
    }
    Ok(count)
}

/// 清理过期的上传临时目录。
pub async fn cleanup_upload_temp(
    storage: &Storage,
    upload: &UploadConfig,
) -> Result<(), std::io::Error> {
    if upload.temp_ttl.is_zero() {
        return Ok(());
    }

    let temp_root = upload_temp_root(storage);
    if fs::metadata(&temp_root).await.is_err() {
        return Ok(());
    }

    let now = SystemTime::now();
    let mut dir = fs::read_dir(&temp_root).await?;
    while let Some(entry) = dir.next_entry().await? {
        let metadata = entry.metadata().await?;
        if !metadata.is_dir() {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let age = match now.duration_since(modified) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if age >= upload.temp_ttl {
            let path = entry.path();
            if let Err(err) = fs::remove_dir_all(&path).await {
                warn!(path = ?path, error = %err, "failed to remove stale upload temp dir");
            } else {
                info!(path = ?path, "removed stale upload temp dir");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use axum::extract::{Extension, Query};
    use axum::http::{HeaderMap, HeaderValue};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::fs;

    use crate::config::{
        DEFAULT_UPLOAD_MAX_CHUNKS, DEFAULT_UPLOAD_MAX_CONCURRENT, DEFAULT_UPLOAD_MAX_SIZE,
        DEFAULT_UPLOAD_TEMP_TTL_SECS,
    };
    use crate::locking::LockManager;

    fn make_storage() -> (tempfile::TempDir, Arc<Storage>) {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("storage");
        std::fs::create_dir_all(&root).expect("create storage root");
        (temp, Arc::new(Storage::new(root)))
    }

    fn make_upload_config() -> Arc<UploadConfig> {
        Arc::new(UploadConfig {
            max_total_size: DEFAULT_UPLOAD_MAX_SIZE,
            max_chunks: DEFAULT_UPLOAD_MAX_CHUNKS,
            max_concurrent: DEFAULT_UPLOAD_MAX_CONCURRENT,
            temp_ttl: Duration::from_secs(DEFAULT_UPLOAD_TEMP_TTL_SECS),
        })
    }

    #[tokio::test]
    async fn init_upload_rejects_traversal_path() {
        let (_temp, storage) = make_storage();
        let upload = make_upload_config();
        let result = init_upload(
            Extension(storage),
            Extension(upload),
            Json(UploadInitRequest {
                name: "../secret.txt".to_string(),
                total_size: 1,
            }),
        )
        .await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }

    #[tokio::test]
    async fn upload_flow_missing_chunk_returns_error() {
        let (_temp, storage) = make_storage();
        let upload = make_upload_config();
        let lock_manager = Arc::new(LockManager::new());
        let JsonResponse(init) = init_upload(
            Extension(storage.clone()),
            Extension(upload.clone()),
            Json(UploadInitRequest {
                name: "file.bin".to_string(),
                total_size: 3,
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("init upload failed"));

        let mut headers = HeaderMap::new();
        headers.insert("X-Chunk-Index", HeaderValue::from_static("1"));
        upload_chunk(
            Query(UploadChunkQuery {
                upload_id: init.upload_id.clone(),
            }),
            headers,
            Extension(storage.clone()),
            Extension(upload.clone()),
            AxumBody::from("abc"),
        )
        .await
        .unwrap_or_else(|_| panic!("upload chunk failed"));

        let result = complete_upload(
            HeaderMap::new(),
            Extension(storage),
            Extension(lock_manager),
            Extension(upload.clone()),
            Json(UploadCompleteRequest {
                upload_id: init.upload_id,
            }),
        )
        .await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }

    #[tokio::test]
    async fn upload_flow_success_cleans_temp_dir() {
        let (temp, storage) = make_storage();
        let upload = make_upload_config();
        let lock_manager = Arc::new(LockManager::new());
        let JsonResponse(init) = init_upload(
            Extension(storage.clone()),
            Extension(upload.clone()),
            Json(UploadInitRequest {
                name: "file.bin".to_string(),
                total_size: 3,
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("init upload failed"));

        let mut headers = HeaderMap::new();
        headers.insert("X-Chunk-Index", HeaderValue::from_static("0"));
        upload_chunk(
            Query(UploadChunkQuery {
                upload_id: init.upload_id.clone(),
            }),
            headers,
            Extension(storage.clone()),
            Extension(upload.clone()),
            AxumBody::from("abc"),
        )
        .await
        .unwrap_or_else(|_| panic!("upload chunk failed"));

        complete_upload(
            HeaderMap::new(),
            Extension(storage.clone()),
            Extension(lock_manager),
            Extension(upload.clone()),
            Json(UploadCompleteRequest {
                upload_id: init.upload_id.clone(),
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("complete upload failed"));

        let file_path = storage.root_path().join("file.bin");
        let contents = fs::read(file_path).await.expect("read file");
        assert_eq!(contents, b"abc");

        let temp_root = temp.path().join(UPLOAD_TEMP_DIR);
        let temp_dir = temp_root.join(init.upload_id);
        assert!(
            fs::metadata(&temp_dir).await.is_err(),
            "upload temp dir should be removed"
        );
    }
}
