//! 嵌入式前端资源服务与 SPA 回退。

use axum::body::Body as AxumBody;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::error::ApiError;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
/// 嵌入式前端构建产物。
pub struct FrontendAssets;

/// 前端资源回退处理器（SPA）。
pub async fn serve_frontend(req: Request<AxumBody>) -> Result<Response, ApiError> {
    let path = req.uri().path().trim_start_matches('/');
    let requested = if path.is_empty() { "index.html" } else { path };
    if let Some(response) = load_embedded_asset(requested)? {
        return Ok(response);
    }

    if !requested.contains('.')
        && let Some(response) = load_embedded_asset("index.html")?
    {
        return Ok(response);
    }

    Err(ApiError::NotFound("not found".into()))
}

/// 加载指定路径的嵌入式资源。
fn load_embedded_asset(path: &str) -> Result<Option<Response>, ApiError> {
    let asset = FrontendAssets::get(path);
    let Some(asset) = asset else {
        return Ok(None);
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.essence_str())
            .map_err(|_| ApiError::Internal("无效的 MIME 类型".into()))?,
    );
    Ok(Some(
        (headers, AxumBody::from(asset.data.into_owned())).into_response(),
    ))
}
