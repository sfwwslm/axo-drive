//! TLS certificate loading and self-signed generation.

use axum_server::tls_rustls::RustlsConfig;
use rcgen::generate_simple_self_signed;
use std::net::IpAddr;
use std::path::PathBuf;
use tokio::fs;
use tracing::info;

use crate::config::Args;

pub async fn build_rustls_config(
    args: &Args,
    host: IpAddr,
) -> Result<RustlsConfig, std::io::Error> {
    let (cert_path, key_path) = if let (Some(cert), Some(key)) = (&args.tls_cert, &args.tls_key) {
        (PathBuf::from(cert), PathBuf::from(key))
    } else {
        generate_self_signed_paths(host)?
    };

    let cert = fs::read(&cert_path).await?;
    let key = fs::read(&key_path).await?;
    RustlsConfig::from_pem(cert, key).await
}

fn generate_self_signed_paths(host: IpAddr) -> Result<(PathBuf, PathBuf), std::io::Error> {
    let cert = generate_simple_self_signed([host.to_string()])
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    let cert_path = std::env::temp_dir().join("axo-drive-cert.pem");
    let key_path = std::env::temp_dir().join("axo-drive-key.pem");
    std::fs::write(&cert_path, cert.cert.pem())?;
    std::fs::write(&key_path, cert.key_pair.serialize_pem())?;
    info!("generated self-signed cert: {:?}", cert_path);
    Ok((cert_path, key_path))
}
