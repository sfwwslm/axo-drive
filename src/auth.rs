//! 认证处理、会话管理与登录限流。

use axum::extract::{Extension, Json, connect_info::ConnectInfo};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::{body::Body as AxumBody, middleware, response::IntoResponse};
use axum_extra::extract::{CookieJar, TypedHeader, cookie::Cookie};
use axum_extra::headers::{Authorization, authorization::Basic};
use cookie::time::Duration as CookieDuration;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

use crate::config::AUTH_COOKIE_NAME;
use crate::error::ApiError;
use crate::http::{RequestScheme, is_https_request, resolve_client_ip};

#[derive(Debug)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    pub sessions: Mutex<HashMap<String, SessionEntry>>,
    pub session_ttl: Duration,
    pub login_attempts: Mutex<HashMap<IpAddr, LoginAttempt>>,
    pub login_window: Duration,
    pub login_max_attempts: u32,
    pub login_lockout: Duration,
}

#[derive(Debug)]
pub struct SessionEntry {
    pub expires_at: Instant,
}

#[derive(Debug)]
pub struct LoginAttempt {
    pub window_start: Instant,
    pub failures: u32,
    pub locked_until: Option<Instant>,
}

/// 认证中间件：校验 Cookie 或 Basic 认证。
pub async fn auth_middleware(
    Extension(auth): Extension<Arc<AuthConfig>>,
    Extension(scheme): Extension<RequestScheme>,
    jar: CookieJar,
    auth_header: Option<TypedHeader<Authorization<Basic>>>,
    req: Request<AxumBody>,
    next: middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    let path = req.uri().path();
    if path.starts_with("/webdav") && !is_https_request(req.headers(), scheme) {
        return Err(ApiError::Forbidden("webdav requires https".into()));
    }
    if is_auth_exempt_path(path) {
        return Ok(next.run(req).await);
    }

    if let Some(cookie) = jar.get(AUTH_COOKIE_NAME)
        && is_session_valid(&auth, cookie.value()).await
    {
        return Ok(next.run(req).await);
    }

    if let Some(TypedHeader(auth_header)) = auth_header
        && auth_header.username() == auth.username
        && auth_header.password() == auth.password
    {
        return Ok(next.run(req).await);
    }

    let mut headers = HeaderMap::new();
    if path.starts_with("/webdav") {
        headers.insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Basic realm="AxoDrive""#),
        );
    }
    Err(ApiError::Unauthorized(headers))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthLoginRequest {
    username: String,
    password: String,
}

/// 登录接口：创建会话并写入 Cookie。
pub async fn auth_login(
    Extension(auth): Extension<Arc<AuthConfig>>,
    Extension(scheme): Extension<RequestScheme>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(payload): Json<AuthLoginRequest>,
) -> Result<(CookieJar, axum::response::Response), ApiError> {
    let client_ip = resolve_client_ip(&headers, Some(addr.ip())).unwrap_or_else(|| addr.ip());

    if let Some(retry_after) = check_login_rate_limit(&auth, client_ip).await {
        return Err(ApiError::TooManyRequests(retry_after));
    }

    if payload.username != auth.username || payload.password != auth.password {
        register_login_failure(&auth, client_ip).await;
        return Err(ApiError::Unauthorized(HeaderMap::new()));
    }

    clear_login_failures(&auth, client_ip).await;

    let token = Uuid::new_v4().to_string();
    let expires_at = Instant::now() + auth.session_ttl;
    let mut sessions = auth.sessions.lock().await;
    sessions.insert(token.clone(), SessionEntry { expires_at });

    let secure = is_https_request(&headers, scheme);
    let cookie = Cookie::build((AUTH_COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(axum_extra::extract::cookie::SameSite::Strict)
        .max_age(CookieDuration::seconds(auth.session_ttl.as_secs() as i64))
        .build();
    let jar = jar.add(cookie);
    Ok((jar, StatusCode::NO_CONTENT.into_response()))
}

/// 登出接口：清理会话并删除 Cookie。
pub async fn auth_logout(
    Extension(auth): Extension<Arc<AuthConfig>>,
    jar: CookieJar,
) -> (CookieJar, StatusCode) {
    if let Some(cookie) = jar.get(AUTH_COOKIE_NAME) {
        remove_session(&auth, cookie.value()).await;
    }

    (
        jar.remove(Cookie::build(AUTH_COOKIE_NAME).path("/").build()),
        StatusCode::NO_CONTENT,
    )
}

fn is_auth_exempt_path(path: &str) -> bool {
    if path == "/api/auth/login"
        || path == "/api/auth/logout"
        || path == "/api/auth/status"
        || path == "/api/version"
    {
        return true;
    }
    if path.starts_with("/api/") || path.starts_with("/webdav") {
        return false;
    }
    true
}

/// 查询当前登录状态。
pub async fn auth_status(
    Extension(auth): Extension<Arc<AuthConfig>>,
    jar: CookieJar,
) -> StatusCode {
    if let Some(cookie) = jar.get(AUTH_COOKIE_NAME)
        && is_session_valid(&auth, cookie.value()).await
    {
        return StatusCode::NO_CONTENT;
    }
    StatusCode::UNAUTHORIZED
}

async fn is_session_valid(auth: &AuthConfig, token: &str) -> bool {
    let mut sessions = auth.sessions.lock().await;
    let now = Instant::now();
    match sessions.get(token) {
        Some(entry) if entry.expires_at > now => true,
        _ => {
            sessions.remove(token);
            false
        }
    }
}

async fn remove_session(auth: &AuthConfig, token: &str) {
    let mut sessions = auth.sessions.lock().await;
    sessions.remove(token);
}

async fn check_login_rate_limit(auth: &AuthConfig, ip: IpAddr) -> Option<u64> {
    if auth.login_max_attempts == 0 {
        return None;
    }

    let mut attempts = auth.login_attempts.lock().await;
    let now = Instant::now();
    let entry = attempts.entry(ip).or_insert(LoginAttempt {
        window_start: now,
        failures: 0,
        locked_until: None,
    });

    if let Some(locked_until) = entry.locked_until {
        if now < locked_until {
            return Some(locked_until.saturating_duration_since(now).as_secs());
        }
        entry.locked_until = None;
        entry.failures = 0;
        entry.window_start = now;
    }

    if now.duration_since(entry.window_start) > auth.login_window {
        entry.window_start = now;
        entry.failures = 0;
    }

    None
}

async fn register_login_failure(auth: &AuthConfig, ip: IpAddr) {
    if auth.login_max_attempts == 0 {
        return;
    }

    let mut attempts = auth.login_attempts.lock().await;
    let now = Instant::now();
    let entry = attempts.entry(ip).or_insert(LoginAttempt {
        window_start: now,
        failures: 0,
        locked_until: None,
    });

    if now.duration_since(entry.window_start) > auth.login_window {
        entry.window_start = now;
        entry.failures = 0;
        entry.locked_until = None;
    }

    entry.failures = entry.failures.saturating_add(1);
    if entry.failures >= auth.login_max_attempts {
        entry.locked_until = Some(now + auth.login_lockout);
        warn!(client_ip = %ip, "login locked out");
    }
}

async fn clear_login_failures(auth: &AuthConfig, ip: IpAddr) {
    let mut attempts = auth.login_attempts.lock().await;
    attempts.remove(&ip);
}

/// 清理过期会话。
pub async fn prune_expired_sessions(auth: &AuthConfig) {
    let mut sessions = auth.sessions.lock().await;
    let now = Instant::now();
    sessions.retain(|_, entry| entry.expires_at > now);
}

/// 清理过期的登录失败记录。
pub async fn prune_login_attempts(auth: &AuthConfig) {
    let mut attempts = auth.login_attempts.lock().await;
    let now = Instant::now();
    attempts.retain(|_, entry| {
        if let Some(locked_until) = entry.locked_until {
            return locked_until > now;
        }
        now.duration_since(entry.window_start) <= auth.login_window
    });
}
