//! API 版本信息处理器。

use axum::response::Json as JsonResponse;
use serde::Serialize;

use crate::error::ApiError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    version: &'static str,
    build_time: &'static str,
    build_env: String,
}

/// 返回当前版本信息。
pub async fn get_version_info() -> Result<JsonResponse<VersionInfo>, ApiError> {
    let version_info = VersionInfo {
        version: crate::build::PKG_VERSION,
        build_time: crate::build::BUILD_TIME,
        build_env: format!(
            "{},{}",
            crate::build::RUST_VERSION,
            crate::build::RUST_CHANNEL
        ),
    };
    Ok(JsonResponse(version_info))
}
