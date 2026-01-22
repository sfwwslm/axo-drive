//! ETag 计算与条件请求校验。

use axum::http::{HeaderMap, header};
use std::fs::Metadata;
use std::time::UNIX_EPOCH;

use crate::error::ApiError;

/// 根据文件元数据生成弱 ETag。
pub fn etag_from_metadata(metadata: &Metadata) -> String {
    let size = metadata.len();
    let modified = metadata.modified().ok();
    if let Some(modified) = modified
        && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
    {
        return format!(
            "W/\"{}-{}-{}\"",
            size,
            duration.as_secs(),
            duration.subsec_nanos()
        );
    }
    format!("W/\"{}\"", size)
}

/// 校验 If-Match / If-None-Match 条件。
pub fn check_preconditions(
    headers: &HeaderMap,
    current_etag: Option<&str>,
    exists: bool,
) -> Result<(), ApiError> {
    if let Some(value) = headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()) {
        if value.trim() == "*" {
            if !exists {
                return Err(ApiError::PreconditionFailed("precondition failed".into()));
            }
        } else if !etag_matches(value, current_etag) {
            return Err(ApiError::PreconditionFailed("precondition failed".into()));
        }
    }

    if let Some(value) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if value.trim() == "*" {
            if exists {
                return Err(ApiError::PreconditionFailed("precondition failed".into()));
            }
        } else if etag_matches(value, current_etag) {
            return Err(ApiError::PreconditionFailed("precondition failed".into()));
        }
    }

    Ok(())
}

fn etag_matches(header_value: &str, current: Option<&str>) -> bool {
    let current = match current {
        Some(value) => value,
        None => return false,
    };
    header_value
        .split(',')
        .map(|item| item.trim())
        .any(|item| item == current)
}
