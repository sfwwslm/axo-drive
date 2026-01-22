//! 统一的 API 错误类型与转换。

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::io::ErrorKind;

use crate::storage::StorageError;

pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
    RangeNotSatisfiable(u64),
    Unauthorized(HeaderMap),
    Forbidden(String),
    PreconditionFailed(String),
    Conflict(String),
    TooManyRequests(u64),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            ApiError::RangeNotSatisfiable(size) => {
                let mut headers = HeaderMap::new();
                if let Ok(value) = HeaderValue::from_str(&format!("bytes */{size}")) {
                    headers.insert(header::CONTENT_RANGE, value);
                }
                (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    headers,
                    "range not satisfiable",
                )
                    .into_response()
            }
            ApiError::Unauthorized(headers) => {
                (StatusCode::UNAUTHORIZED, headers, "unauthorized").into_response()
            }
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg).into_response(),
            ApiError::PreconditionFailed(msg) => {
                (StatusCode::PRECONDITION_FAILED, msg).into_response()
            }
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg).into_response(),
            ApiError::TooManyRequests(retry_after) => {
                let mut headers = HeaderMap::new();
                if retry_after > 0
                    && let Ok(value) = HeaderValue::from_str(&retry_after.to_string())
                {
                    headers.insert(header::RETRY_AFTER, value);
                }
                (StatusCode::TOO_MANY_REQUESTS, headers, "too many requests").into_response()
            }
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::InvalidPath => ApiError::BadRequest("invalid path".into()),
            StorageError::Io(err) => match err.kind() {
                ErrorKind::NotFound => ApiError::NotFound(err.to_string()),
                _ => ApiError::Internal(err.to_string()),
            },
        }
    }
}
