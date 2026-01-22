//! CLI arguments and server configuration defaults.

use clap::Parser;
use shadow_rs::formatcp;

use crate::build;

const VERSION_INFO: &str = formatcp!(
    r#"{}\ncommit_hash: {}\nbuild_time: {}\nbuild_env: {},{}"#,
    build::PKG_VERSION,
    build::SHORT_COMMIT,
    build::BUILD_TIME,
    build::RUST_VERSION,
    build::RUST_CHANNEL
);

pub const MAX_CHUNK_SIZE: u64 = 16 * 1024 * 1024;
pub const UPLOAD_TEMP_DIR: &str = ".axo/temp";
pub const DEFAULT_AUTH_USER: &str = "axo";
pub const DEFAULT_AUTH_PASS: &str = "axo";
pub const AUTH_COOKIE_NAME: &str = "AXO_SESSION";
pub const DEFAULT_SESSION_TTL_SECS: u64 = 24 * 60 * 60;
pub const DEFAULT_LOGIN_MAX_ATTEMPTS: u32 = 5;
pub const DEFAULT_LOGIN_WINDOW_SECS: u64 = 5 * 60;
pub const DEFAULT_LOGIN_LOCKOUT_SECS: u64 = 10 * 60;
pub const DEFAULT_UPLOAD_MAX_SIZE: u64 = 100 * 1024 * 1024 * 1024;
pub const DEFAULT_UPLOAD_MAX_CHUNKS: u64 = 8192;
pub const DEFAULT_UPLOAD_MAX_CONCURRENT: u64 = 8;
pub const DEFAULT_UPLOAD_TEMP_TTL_SECS: u64 = 24 * 60 * 60;
pub const SESSION_PRUNE_INTERVAL_SECS: u64 = 300;
pub const UPLOAD_CLEAN_INTERVAL_SECS: u64 = 900;

/// CLI arguments and environment configuration for the server.
#[derive(Parser, Debug)]
#[command(name = "axo-drive", version = VERSION_INFO, about = "AxoDrive server")]
pub struct Args {
    #[arg(
        short = 's',
        long,
        env = "AXO_STORAGE_DIR",
        default_value = ".axo/storage",
        help = "Storage directory for files"
    )]
    pub storage_dir: String,
    #[arg(
        long,
        env = "AXO_AUTH_USER",
        default_value = DEFAULT_AUTH_USER,
        help = "Auth username for Web UI/WebDAV"
    )]
    pub auth_user: String,
    #[arg(
        long,
        env = "AXO_AUTH_PASS",
        default_value = DEFAULT_AUTH_PASS,
        help = "Auth password for Web UI/WebDAV"
    )]
    pub auth_pass: String,
    #[arg(
        short = 'b',
        long,
        env = "AXO_BIND",
        default_value = "0.0.0.0",
        help = "Bind address for HTTP/HTTPS"
    )]
    pub host: String,
    #[arg(
        short = 'p',
        long,
        env = "AXO_HTTP_PORT",
        default_value_t = 5005,
        help = "HTTP port"
    )]
    pub http_port: u16,
    #[arg(
        short = 'P',
        long,
        env = "AXO_HTTPS_PORT",
        default_value_t = 5006,
        help = "HTTPS port"
    )]
    pub https_port: u16,
    #[arg(short = 'c', long, env = "AXO_TLS_CERT", help = "TLS cert path")]
    pub tls_cert: Option<String>,
    #[arg(short = 'k', long, env = "AXO_TLS_KEY", help = "TLS key path")]
    pub tls_key: Option<String>,
    #[arg(long, env = "AXO_CORS_ORIGINS", help = "Comma separated CORS origins")]
    pub cors_origins: Option<String>,
    #[arg(
        long,
        env = "AXO_SESSION_TTL_SECS",
        default_value_t = DEFAULT_SESSION_TTL_SECS,
        help = "Session expiration in seconds"
    )]
    pub session_ttl_secs: u64,
    #[arg(
        long,
        env = "AXO_LOGIN_MAX_ATTEMPTS",
        default_value_t = DEFAULT_LOGIN_MAX_ATTEMPTS,
        help = "Max login attempts before lockout"
    )]
    pub login_max_attempts: u32,
    #[arg(
        long,
        env = "AXO_LOGIN_WINDOW_SECS",
        default_value_t = DEFAULT_LOGIN_WINDOW_SECS,
        help = "Login attempt window in seconds"
    )]
    pub login_window_secs: u64,
    #[arg(
        long,
        env = "AXO_LOGIN_LOCKOUT_SECS",
        default_value_t = DEFAULT_LOGIN_LOCKOUT_SECS,
        help = "Login lockout time after max attempts"
    )]
    pub login_lockout_secs: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_SIZE",
        default_value_t = DEFAULT_UPLOAD_MAX_SIZE,
        help = "Max upload total size in bytes (0 to disable)"
    )]
    pub upload_max_size: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_CHUNKS",
        default_value_t = DEFAULT_UPLOAD_MAX_CHUNKS,
        help = "Max chunks per upload (0 to disable)"
    )]
    pub upload_max_chunks: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_MAX_CONCURRENT",
        default_value_t = DEFAULT_UPLOAD_MAX_CONCURRENT,
        help = "Max concurrent uploads (0 to disable)"
    )]
    pub upload_max_concurrent: u64,
    #[arg(
        long,
        env = "AXO_UPLOAD_TEMP_TTL_SECS",
        default_value_t = DEFAULT_UPLOAD_TEMP_TTL_SECS,
        help = "Upload temp cleanup threshold in seconds (0 to disable)"
    )]
    pub upload_temp_ttl_secs: u64,
}
