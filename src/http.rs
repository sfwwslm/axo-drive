//! HTTP 辅助工具：请求方案识别、CORS 与安全头。

use axum::body::Body as AxumBody;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::{middleware, response::Response};
use std::net::IpAddr;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::warn;

#[derive(Clone, Copy, Debug)]
pub enum RequestScheme {
    Http,
    Https,
}

impl RequestScheme {
    /// 判断请求是否为 HTTPS。
    pub fn is_https(self) -> bool {
        matches!(self, RequestScheme::Https)
    }
}

/// 构建 CORS Layer（支持逗号分隔的来源列表）。
pub fn build_cors_layer(cors_origins: Option<&str>) -> Option<CorsLayer> {
    let origins = cors_origins?
        .split(',')
        .map(|origin| origin.trim())
        .filter(|origin| !origin.is_empty())
        .filter_map(|origin| match HeaderValue::from_str(origin) {
            Ok(value) => Some(value),
            Err(_) => {
                warn!(origin, "invalid cors origin");
                None
            }
        })
        .collect::<Vec<_>>();

    if origins.is_empty() {
        return None;
    }

    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(true),
    )
}

/// 从 `x-forwarded-for` 解析客户端 IP。
pub fn extract_forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<IpAddr>().ok())
}

/// 综合转发头与连接信息计算客户端 IP。
pub fn resolve_client_ip(headers: &HeaderMap, connect_ip: Option<IpAddr>) -> Option<IpAddr> {
    extract_forwarded_ip(headers).or(connect_ip)
}

/// 判断请求是否为 HTTPS（含反向代理头）。
pub fn is_https_request(headers: &HeaderMap, scheme: RequestScheme) -> bool {
    if let Some(value) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        return value.eq_ignore_ascii_case("https");
    }
    scheme.is_https()
}

/// 添加基础安全响应头。
pub async fn add_security_headers(
    request: Request<AxumBody>,
    next: middleware::Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
