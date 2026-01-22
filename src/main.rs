//! AxoDrive server binary.
//!
//! This crate wires together HTTP/WebDAV routing, authentication, upload
//! handling, and static frontend delivery. The main entry point builds the
//! Axum router, configures TLS, and starts HTTP/HTTPS listeners.

mod auth;
mod background;
mod config;
mod error;
mod files;
mod frontend;
mod http;
mod logging;
mod storage;
mod tls;
mod upload;
mod version;
mod webdav;

use axum::extract::{DefaultBodyLimit, Extension, connect_info::ConnectInfo};
use axum::http::Request;
use axum::routing::{any, delete, get, patch, post, put};
use axum::{Router, middleware};
use axum_server::Handle;
use clap::Parser;
use dav_server::{DavHandler, fakels::FakeLs, localfs::LocalFs};
use shadow_rs::shadow;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::Mutex;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::{Level, info, info_span};

use crate::auth::AuthConfig;
use crate::background::spawn_background_tasks;
use crate::config::Args;
use crate::http::{RequestScheme, build_cors_layer};
use crate::storage::Storage;
use crate::upload::UploadConfig;

shadow!(build);

/// Starts the AxoDrive server and blocks until shutdown.
#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    logging::init_logging();

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
        .route("/webdav", any(webdav::webdav_handler))
        .route("/webdav/{*path}", any(webdav::webdav_handler))
        .route("/api/files/list", get(files::list_files))
        .route("/api/files/download", get(files::download_file))
        .route("/api/files/write", put(files::write_file))
        .route("/api/files/delete", delete(files::delete_entry))
        .route("/api/files/mkdir", post(files::create_directory))
        .route("/api/upload/init", post(upload::init_upload))
        .route(
            "/api/upload/chunk",
            patch(upload::upload_chunk).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/upload/complete", post(upload::complete_upload))
        .route("/api/upload/abort", post(upload::abort_upload))
        .route("/api/auth/login", post(auth::auth_login))
        .route("/api/auth/logout", post(auth::auth_logout))
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/version", get(version::get_version_info))
        .fallback(frontend::serve_frontend)
        .layer(middleware::from_fn(auth::auth_middleware))
        .layer(middleware::from_fn(http::add_security_headers))
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
    let tls_config = tls::build_rustls_config(&args, host).await?;
    let handle = Handle::new();

    info!("🚀 Starting HTTP server at {}", http_addr);
    info!("🔒 Starting HTTPS server at {}", https_addr);

    let http_app = app.clone().layer(Extension(RequestScheme::Http));
    let https_app = app.layer(Extension(RequestScheme::Https));

    let http_server = axum_server::bind(http_addr)
        .handle(handle.clone())
        .serve(http_app.into_make_service_with_connect_info::<SocketAddr>());
    let https_server = axum_server::bind_rustls(https_addr, tls_config)
        .handle(handle.clone())
        .serve(https_app.into_make_service_with_connect_info::<SocketAddr>());

    spawn_background_tasks(storage_for_tasks, auth_for_tasks, upload_for_tasks);
    tokio::select! {
        result = http_server => result?,
        result = https_server => result?,
        _ = shutdown_signal(handle) => {}
    }

    Ok(())
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
