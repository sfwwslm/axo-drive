//! AxoDrive server binary.
//!
//! This crate wires together HTTP/WebDAV routing, authentication, upload
//! handling, and static frontend delivery. The main entry point builds the
//! Axum router, configures TLS, and starts HTTP/HTTPS listeners.

mod storage;

use axum::extract::{DefaultBodyLimit, Extension, Json, Query, connect_info::ConnectInfo};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Json as JsonResponse, Response};
use axum::routing::{any, delete, get, patch, post, put};
use axum::{Error as AxumError, Router, body::Body as AxumBody, middleware};
use axum_extra::extract::{CookieJar, TypedHeader, cookie::Cookie};
use axum_extra::headers::{Authorization, authorization::Basic};
use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use cookie::time::Duration as CookieDuration;
use dav_server::{DavHandler, body::Body as DavBody, fakels::FakeLs, localfs::LocalFs};
use futures_util::stream::StreamExt;
use http_body_util::BodyExt;
use httpdate::{fmt_http_date, parse_http_date};
use rcgen::generate_simple_self_signed;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use shadow_rs::{formatcp, shadow};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{ErrorKind, SeekFrom};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use storage::{FileEntry, Storage, StorageError};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::signal;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::{Level, debug, info, info_span, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

shadow!(build);

const VERSION_INFO: &str = formatcp!(
    r#"{}
commit_hash: {}
build_time: {}
build_env: {},{}"#,
    build::PKG_VERSION,
    build::SHORT_COMMIT,
    build::BUILD_TIME,
    build::RUST_VERSION,
    build::RUST_CHANNEL
);

const MAX_CHUNK_SIZE: u64 = 16 * 1024 * 1024;
const UPLOAD_TEMP_DIR: &str = ".axo/temp";
const DEFAULT_AUTH_USER: &str = "axo";
const DEFAULT_AUTH_PASS: &str = "axo";
const AUTH_COOKIE_NAME: &str = "AXO_SESSION";
const DEFAULT_SESSION_TTL_SECS: u64 = 24 * 60 * 60;
const DEFAULT_LOGIN_MAX_ATTEMPTS: u32 = 5;
const DEFAULT_LOGIN_WINDOW_SECS: u64 = 5 * 60;
const DEFAULT_LOGIN_LOCKOUT_SECS: u64 = 10 * 60;
const DEFAULT_UPLOAD_MAX_SIZE: u64 = 100 * 1024 * 1024 * 1024;
const DEFAULT_UPLOAD_MAX_CHUNKS: u64 = 8192;
const DEFAULT_UPLOAD_MAX_CONCURRENT: u64 = 8;
const DEFAULT_UPLOAD_TEMP_TTL_SECS: u64 = 24 * 60 * 60;
const SESSION_PRUNE_INTERVAL_SECS: u64 = 300;
const UPLOAD_CLEAN_INTERVAL_SECS: u64 = 900;

/// CLI arguments and environment configuration for the server.
#[derive(Parser, Debug)]
#[command(name = "axo-drive", version = VERSION_INFO, about = "AxoDrive server")]
struct Args {
    #[arg(
        short = 's',
        long,
        env = "AXO_STORAGE_DIR",
        default_value = ".axo/storage",
        help = "Storage directory for files"
    )]
    storage_dir: String,
    #[arg(
        long,
        env = "AXO_AUTH_USER",
        default_value = DEFAULT_AUTH_USER,
        help = "Auth username for Web UI/WebDAV"
    )]
    auth_user: String,
    #[arg(
        long,
        env = "AXO_AUTH_PASS",
        default_value = DEFAULT_AUTH_PASS,
        help = "Auth password for Web UI/WebDAV"
    )]
    auth_pass: String,
    #[arg(
        short = 'b',
        long,
        env = "AXO_BIND",
        default_value = "0.0.0.0",
        help = "Bind address for HTTP/HTTPS"
    )]
    host: String,
    #[arg(
        short = 'p',
        long,
        env = "AXO_HTTP_PORT",
        default_value_t = 5005,
        help = "HTTP port"
    )]
    http_port: u16,
    #[arg(
        short = 'P',
        long,
        env = "AXO_HTTPS_PORT",
        default_value_t = 5006,
        help = "HTTPS port"
    )]
    https_port: u16,
    #[arg(long, env = "AXO_TLS_CERT", help = "TLS certificate path (PEM)")]
    tls_cert: Option<String>,
    #[arg(long, env = "AXO_TLS_KEY", help = "TLS private key path (PEM)")]
    tls_key: Option<String>,
    #[arg(
        long,
        env = "AXO_SESSION_TTL_SECS",
        default_value_t = DEFAULT_SESSION_TTL_SECS,
        help = "Session TTL in seconds"
    )]
    session_ttl_secs: u64,
    #[arg(
        long,
        env = "AXO_LOGIN_MAX_ATTEMPTS",
        default_value_t = DEFAULT_LOGIN_MAX_ATTEMPTS,
        help = "Max login attempts within window (0 to disable)"
    )]
    login_max_attempts: u32,
    #[arg(
        long,
        env = "AXO_LOGIN_WINDOW_SECS",
        default_value_t = DEFAULT_LOGIN_WINDOW_SECS,
        help = "Login rate-limit window in seconds"
    )]
    login_window_secs: u64,
    #[arg(
        long,
        env = "AXO_LOGIN_LOCKOUT_SECS",
        default_value_t = DEFAULT_LOGIN_LOCKOUT_SECS,
        help = "Login lockout time after max attempts"
    )]
    login_lockout_secs: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_SIZE",
        default_value_t = DEFAULT_UPLOAD_MAX_SIZE,
        help = "Max upload total size in bytes (0 to disable)"
    )]
    upload_max_size: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_CHUNKS",
        default_value_t = DEFAULT_UPLOAD_MAX_CHUNKS,
        help = "Max chunks per upload (0 to disable)"
    )]
    upload_max_chunks: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_CONCURRENT",
        default_value_t = DEFAULT_UPLOAD_MAX_CONCURRENT,
        help = "Max concurrent uploads (0 to disable)"
    )]
    upload_max_concurrent: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_TEMP_TTL_SECS",
        default_value_t = DEFAULT_UPLOAD_TEMP_TTL_SECS,
        help = "Upload temp cleanup threshold in seconds (0 to disable)"
    )]
    upload_temp_ttl_secs: u64,
    #[arg(
        long,
        env = "AXO_CORS_ORIGINS",
        help = "Comma-separated CORS origins (e.g. https://example.com,https://localhost:5173)"
    )]
    cors_origins: Option<String>,
}

/// Authentication and session configuration shared by handlers.
struct AuthConfig {
    username: String,
    password: String,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    session_ttl: Duration,
    login_attempts: Mutex<HashMap<IpAddr, LoginAttempt>>,
    login_window: Duration,
    login_max_attempts: u32,
    login_lockout: Duration,
}

/// Tracks a single active session and its expiration time.
struct SessionEntry {
    expires_at: Instant,
}

/// State for rate-limiting failed login attempts per IP.
struct LoginAttempt {
    window_start: Instant,
    failures: u32,
    locked_until: Option<Instant>,
}

/// Upload limits and cleanup settings shared by handlers.
struct UploadConfig {
    max_total_size: u64,
    max_chunks: u64,
    max_concurrent: u64,
    temp_ttl: Duration,
}

#[derive(Serialize)]
/// Build and version metadata returned by the version API.
struct VersionInfo {
    version: &'static str,
    build_time: &'static str,
    build_env: String,
}

#[derive(Clone, Copy)]
/// Request scheme marker used to construct absolute URLs.
enum RequestScheme {
    Http,
    Https,
}

impl RequestScheme {
    /// Returns true when the request was served over HTTPS.
    fn is_https(self) -> bool {
        matches!(self, RequestScheme::Https)
    }
}

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
/// Embedded frontend build artifacts served by the fallback handler.
struct FrontendAssets;

/// Starts the AxoDrive server and blocks until shutdown.
#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    init_logging();

    let args = Args::parse();
    let storage_dir = args.storage_dir.clone();
    let storage = Arc::new(Storage::new(PathBuf::from(storage_dir)));
    let auth_config = Arc::new(AuthConfig {
        username: args.auth_user.clone(),
        password: args.auth_pass.clone(),
        sessions: Mutex::new(HashMap::new()),
        session_ttl: Duration::from_secs(args.session_ttl_secs),
        login_attempts: Mutex::new(HashMap::new()),
        login_window: Duration::from_secs(args.login_window_secs),
        login_max_attempts: args.login_max_attempts,
        login_lockout: Duration::from_secs(args.login_lockout_secs),
    });
    let upload_config = Arc::new(UploadConfig {
        max_total_size: args.upload_max_size,
        max_chunks: args.upload_max_chunks,
        max_concurrent: args.upload_max_concurrent,
        temp_ttl: Duration::from_secs(args.upload_temp_ttl_secs),
    });
    let storage_for_tasks = storage.clone();
    let auth_for_tasks = auth_config.clone();
    let upload_for_tasks = upload_config.clone();
    storage.ensure_root().await?;
    let dav_handler = Arc::new(
        DavHandler::builder()
            .strip_prefix("/webdav")
            .filesystem(LocalFs::new(storage.root_path(), false, false, false))
            .locksystem(FakeLs::new())
            .build_handler(),
    );

    let mut app = Router::new()
        .route("/webdav", any(webdav_handler))
        .route("/webdav/{*path}", any(webdav_handler))
        .route("/api/files/list", get(list_files))
        .route("/api/files/download", get(download_file))
        .route("/api/files/write", put(write_file))
        .route("/api/files/delete", delete(delete_entry))
        .route("/api/files/mkdir", post(create_directory))
        .route("/api/upload/init", post(init_upload))
        .route(
            "/api/upload/chunk",
            patch(upload_chunk).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/upload/complete", post(complete_upload))
        .route("/api/upload/abort", post(abort_upload))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/status", get(auth_status))
        .route("/api/version", get(get_version_info))
        .fallback(serve_frontend)
        .layer(middleware::from_fn(auth_middleware))
        .layer(middleware::from_fn(add_security_headers))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let forwarded_ip = request
                        .headers()
                        .get("x-forwarded-for")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
                    let connect_ip = request
                        .extensions()
                        .get::<ConnectInfo<SocketAddr>>()
                        .map(|ConnectInfo(addr)| addr.to_string());
                    let client_ip = forwarded_ip
                        .or(connect_ip)
                        .unwrap_or_else(|| "unknown".to_string());

                    info_span!(
                        env!("CARGO_CRATE_NAME"),
                        client_ip,
                        method = ?request.method(),
                        path = ?request.uri().path(),
                        some_other_field = tracing::field::Empty,
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::DEBUG))
                .on_response(DefaultOnResponse::new().level(Level::DEBUG)),
        )
        .layer(Extension(storage))
        .layer(Extension(auth_config))
        .layer(Extension(upload_config))
        .layer(Extension(dav_handler));

    if let Some(cors_layer) = build_cors_layer(args.cors_origins.as_deref()) {
        app = app.layer(cors_layer);
    }

    let host = args
        .host
        .parse::<IpAddr>()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string()))?;
    let http_addr = SocketAddr::new(host, args.http_port);
    let https_addr = SocketAddr::new(host, args.https_port);
    let tls_config = build_rustls_config(&args, host).await?;
    let handle = Handle::new();

    info!("🚀 Starting HTTP server at {}", http_addr);
    info!("🔒 Starting HTTPS server at {}", https_addr);

    let http_app = app.clone().layer(Extension(RequestScheme::Http));
    let https_app = app.layer(Extension(RequestScheme::Https));

    spawn_background_tasks(storage_for_tasks, auth_for_tasks, upload_for_tasks);
    tokio::spawn(shutdown_signal(handle.clone()));

    let http_server = axum_server::bind(http_addr)
        .handle(handle.clone())
        .serve(http_app.into_make_service_with_connect_info::<SocketAddr>());
    let https_server = axum_server::bind_rustls(https_addr, tls_config)
        .handle(handle)
        .serve(https_app.into_make_service_with_connect_info::<SocketAddr>());

    tokio::try_join!(http_server, https_server)?;
    Ok(())
}

/// Build a TLS config from provided cert/key or a generated self-signed pair.
async fn build_rustls_config(args: &Args, host: IpAddr) -> Result<RustlsConfig, std::io::Error> {
    if let (Some(cert), Some(key)) = (&args.tls_cert, &args.tls_key) {
        info!(
            cert = cert.as_str(),
            key = key.as_str(),
            "use tls certificate"
        );
        return RustlsConfig::from_pem_file(cert, key)
            .await
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err));
    }

    let (cert_path, key_path) = generate_self_signed_paths(host)?;
    info!(
        cert = ?cert_path,
        key = ?key_path,
        "generated self-signed certificate"
    );
    RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

/// Create temporary self-signed certificate and key files for the given host.
fn generate_self_signed_paths(host: IpAddr) -> Result<(PathBuf, PathBuf), std::io::Error> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    match host {
        IpAddr::V4(addr) if !addr.is_unspecified() => names.push(addr.to_string()),
        IpAddr::V6(addr) if !addr.is_unspecified() => names.push(addr.to_string()),
        _ => {}
    }

    let certified =
        generate_simple_self_signed(names).map_err(|err| std::io::Error::other(err.to_string()))?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();

    let dir = std::env::temp_dir().join(format!("axo-drive-cert-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert_pem)?;
    std::fs::write(&key_path, key_pem)?;
    Ok((cert_path, key_path))
}

fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // axum logs rejections from built-in extractors with the `axum::rejection`
                // target, at `TRACE` level. `axum::rejection=trace` enables showing those events
                format!(
                    "{}=info,tower_http=info,axum::rejection=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn shutdown_signal(handle: Handle) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Received termination signal shutting down");
    handle.graceful_shutdown(Some(Duration::from_secs(10)));
}

fn build_cors_layer(cors_origins: Option<&str>) -> Option<CorsLayer> {
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

fn extract_forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<IpAddr>().ok())
}

fn resolve_client_ip(headers: &HeaderMap, connect_ip: Option<IpAddr>) -> Option<IpAddr> {
    extract_forwarded_ip(headers).or(connect_ip)
}

fn is_https_request(headers: &HeaderMap, scheme: RequestScheme) -> bool {
    if let Some(value) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        return value.eq_ignore_ascii_case("https");
    }
    scheme.is_https()
}

fn spawn_background_tasks(storage: Arc<Storage>, auth: Arc<AuthConfig>, upload: Arc<UploadConfig>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SESSION_PRUNE_INTERVAL_SECS));
        loop {
            interval.tick().await;
            prune_expired_sessions(&auth).await;
            prune_login_attempts(&auth).await;
        }
    });

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(UPLOAD_CLEAN_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(err) = cleanup_upload_temp(&storage, &upload).await {
                warn!(error = %err, "upload temp cleanup failed");
            }
        }
    });
}

async fn prune_expired_sessions(auth: &AuthConfig) {
    let mut sessions = auth.sessions.lock().await;
    let now = Instant::now();
    sessions.retain(|_, entry| entry.expires_at > now);
}

async fn prune_login_attempts(auth: &AuthConfig) {
    let mut attempts = auth.login_attempts.lock().await;
    let now = Instant::now();
    attempts.retain(|_, entry| {
        if let Some(locked_until) = entry.locked_until {
            return locked_until > now;
        }
        now.duration_since(entry.window_start) <= auth.login_window
    });
}

async fn cleanup_upload_temp(
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

async fn auth_middleware(
    Extension(auth): Extension<Arc<AuthConfig>>,
    Extension(scheme): Extension<RequestScheme>,
    jar: CookieJar,
    auth_header: Option<TypedHeader<Authorization<Basic>>>,
    req: Request<AxumBody>,
    next: middleware::Next,
) -> Result<Response, ApiError> {
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
struct AuthLoginRequest {
    username: String,
    password: String,
}

async fn auth_login(
    Extension(auth): Extension<Arc<AuthConfig>>,
    Extension(scheme): Extension<RequestScheme>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(payload): Json<AuthLoginRequest>,
) -> Result<(CookieJar, Response), ApiError> {
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

async fn auth_logout(
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

async fn auth_status(Extension(auth): Extension<Arc<AuthConfig>>, jar: CookieJar) -> StatusCode {
    if let Some(cookie) = jar.get(AUTH_COOKIE_NAME)
        && is_session_valid(&auth, cookie.value()).await
    {
        return StatusCode::NO_CONTENT;
    }
    StatusCode::UNAUTHORIZED
}

async fn get_version_info() -> Result<JsonResponse<VersionInfo>, ApiError> {
    let version_info = VersionInfo {
        version: build::PKG_VERSION,
        build_time: build::BUILD_TIME,
        build_env: format!("{},{}", build::RUST_VERSION, build::RUST_CHANNEL),
    };
    Ok(JsonResponse(version_info))
}

async fn add_security_headers(
    request: Request<AxumBody>,
    next: middleware::Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

#[derive(Deserialize)]
struct OptionalPathQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
struct RequiredPathQuery {
    path: String,
}

#[derive(Deserialize)]
struct DirCreateBody {
    path: String,
}

async fn list_files(
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

async fn download_file(
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

async fn write_file(
    Query(RequiredPathQuery { path }): Query<RequiredPathQuery>,
    Extension(storage): Extension<Arc<Storage>>,
    body: AxumBody,
) -> Result<StatusCode, ApiError> {
    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }
    info!(path, "write file");

    let target = storage.resolve_path_checked(&path, true).await?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    }
    let mut file = File::create(&target)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    let mut data_stream = BodyExt::into_data_stream(body);
    while let Some(chunk) = data_stream.next().await {
        let chunk = chunk.map_err(|err: AxumError| ApiError::Internal(err.to_string()))?;
        if !chunk.is_empty() {
            file.write_all(&chunk)
                .await
                .map_err(|err| ApiError::Internal(err.to_string()))?;
        }
    }
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadInitRequest {
    name: String,
    total_size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadInitResponse {
    upload_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadMetadata {
    name: String,
    total_size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadChunkQuery {
    upload_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadCompleteRequest {
    upload_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadAbortRequest {
    upload_id: String,
}

async fn init_upload(
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

async fn upload_chunk(
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

async fn complete_upload(
    Extension(storage): Extension<Arc<Storage>>,
    Extension(upload): Extension<Arc<UploadConfig>>,
    Json(payload): Json<UploadCompleteRequest>,
) -> Result<StatusCode, ApiError> {
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

    let target = storage.resolve_path_checked(&metadata.name, true).await?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    }
    let mut output = File::create(&target)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    let mut total_written: u64 = 0;
    for (_, path) in &parts {
        let mut part_file = File::open(path)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let copied = tokio::io::copy(&mut part_file, &mut output)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        total_written += copied;
    }

    if metadata.total_size > 0 && total_written != metadata.total_size {
        warn!(
            upload_id = payload.upload_id,
            expected = metadata.total_size,
            actual = total_written,
            "size mismatch after merge"
        );
        return Err(ApiError::BadRequest("size mismatch".into()));
    }

    fs::remove_dir_all(&temp_dir)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    info!(
        upload_id = payload.upload_id,
        name = metadata.name,
        total_size = metadata.total_size,
        "upload complete"
    );
    Ok(StatusCode::CREATED)
}

async fn delete_entry(
    Query(RequiredPathQuery { path }): Query<RequiredPathQuery>,
    Extension(storage): Extension<Arc<Storage>>,
) -> Result<StatusCode, ApiError> {
    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }
    storage.delete_path(&path).await?;
    info!(path, "delete entry");
    Ok(StatusCode::NO_CONTENT)
}

async fn create_directory(
    Extension(storage): Extension<Arc<Storage>>,
    payload: Json<DirCreateBody>,
) -> Result<StatusCode, ApiError> {
    let DirCreateBody { path } = payload.0;

    if path.is_empty() {
        return Err(ApiError::BadRequest("path is required".into()));
    }

    storage.create_dir(&path).await?;
    info!(path, "create directory");
    Ok(StatusCode::CREATED)
}

async fn abort_upload(
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

async fn webdav_handler(
    Extension(dav_handler): Extension<Arc<DavHandler>>,
    req: Request<AxumBody>,
) -> Response<DavBody> {
    dav_handler.handle(req).await
}

async fn serve_frontend(req: Request<AxumBody>) -> Result<Response, ApiError> {
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

fn upload_temp_root(storage: &Storage) -> PathBuf {
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

async fn count_upload_temp_dirs(storage: &Storage) -> Result<u64, ApiError> {
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

enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
    RangeNotSatisfiable(u64),
    Unauthorized(HeaderMap),
    Forbidden(String),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use axum::extract::{Extension, Query};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::fs;

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
    async fn write_file_rejects_traversal_path() {
        let (_temp, storage) = make_storage();
        let result = write_file(
            Query(RequiredPathQuery {
                path: "../secret.txt".to_string(),
            }),
            Extension(storage),
            AxumBody::from("data"),
        )
        .await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }

    #[tokio::test]
    async fn upload_flow_missing_chunk_returns_error() {
        let (_temp, storage) = make_storage();
        let upload = make_upload_config();
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
            Extension(storage),
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
            Extension(storage.clone()),
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
