# AxoDrive

A lightweight, zero-runtime-dependency LAN file manager with Web UI + WebDAV support.

[中文](README.md)

## Highlights

- Web UI: file list, upload/download, delete, create directory, progress
- WebDAV: mount as a network drive (shares the same storage root as the HTTP API)
- Chunked uploads: `init -> chunk -> complete` flow with concurrency and retries
- Resumable downloads: Range requests + If-Range streaming
- Built-in auth: cookie session for Web UI, Basic Auth for WebDAV
- Auto locale: Chinese for zh browsers, English otherwise
- Single binary distribution: embeds `frontend/dist` into the Rust binary

## Layout

```text
axo-drive/
├── Cargo.toml
├── src/
│   └── main.rs
├── frontend/
│   ├── src/
│   ├── public/
│   └── dist/            # build output (embedded)
├── build.rs             # build metadata (shadow-rs)
├── Dockerfile
└── scripts/
    ├── build.sh
    └── build_docker.sh
```

## Quick Start

### Run the backend

```bash
cargo run
```

Starts both by default:

- HTTP: `http://0.0.0.0:5005`
- HTTPS: `https://0.0.0.0:5006` (auto generates a self-signed cert if none provided)

### Frontend development

```bash
cd frontend && pnpm dev
```

### Build for release

```bash
cd frontend && pnpm build
cargo build --release
```

Cross-platform build script (Linux musl + Windows gnu):

```bash
./scripts/build.sh
```

## Configuration

Supports CLI args and environment variables (CLI takes precedence):

- `--storage-dir` / `AXO_STORAGE_DIR`: storage directory (default `.axo/storage`)
- `--auth-user` / `AXO_AUTH_USER`: auth username (default `axo`)
- `--auth-pass` / `AXO_AUTH_PASS`: auth password (default `axo`)
- `--host` / `AXO_BIND`: bind address (default `0.0.0.0`)
- `--http-port` / `AXO_HTTP_PORT`: HTTP port (default `5005`)
- `--https-port` / `AXO_HTTPS_PORT`: HTTPS port (default `5006`)
- `--tls-cert` / `AXO_TLS_CERT`: TLS cert path (PEM)
- `--tls-key` / `AXO_TLS_KEY`: TLS private key path (PEM)
- `--session-ttl-secs` / `AXO_SESSION_TTL_SECS`: session TTL (default 86400s)
- `--login-max-attempts` / `AXO_LOGIN_MAX_ATTEMPTS`: max login attempts (default 5, 0 disables)
- `--login-window-secs` / `AXO_LOGIN_WINDOW_SECS`: login window (default 300s)
- `--login-lockout-secs` / `AXO_LOGIN_LOCKOUT_SECS`: lockout duration (default 600s)
- `--upload-max-size` / `AXO_UPLOAD_MAX_SIZE`: max upload size (default 100GiB, 0 unlimited)
- `--upload-max-chunks` / `AXO_UPLOAD_MAX_CHUNKS`: max chunks per upload (default 8192, 0 unlimited)
- `--upload-max-concurrent` / `AXO_UPLOAD_MAX_CONCURRENT`: max concurrent uploads (default 8, 0 unlimited)
- `--upload-temp-ttl-secs` / `AXO_UPLOAD_TEMP_TTL_SECS`: temp cleanup threshold (default 86400s, 0 disables)
- `--cors-origins` / `AXO_CORS_ORIGINS`: allowed CORS origins (comma separated)

If `AXO_CORS_ORIGINS` is unset, no CORS headers are added. Configure it when serving the frontend from a different origin.

### CLI usage

```bash
cargo run -- --help
```

Example:

```bash
cargo run -- -b 0.0.0.0 -p 8080 -P 8443 -s /data/axo-drive --auth-user axo --auth-pass axo
```

## Security

- WebDAV is HTTPS-only to avoid Basic Auth over plaintext.
- Web UI cookies are HttpOnly, SameSite=Strict, and Secure on HTTPS.
- Sessions are TTL-bounded and pruned periodically.
- Login rate limiting and lockout are enabled.
- Upload size/chunk/concurrency limits are enforced.
- Expired upload temp folders are cleaned on schedule.

## API Overview

### File operations

- `GET /api/files/list?path=`: list directory
- `GET /api/files/download?path=`: download (supports Range)
- `PUT /api/files/write?path=`: write file directly
- `DELETE /api/files/delete?path=`: delete file or directory
- `POST /api/files/mkdir`: create directory

### Chunked uploads

- `POST /api/upload/init` `{ name, totalSize } -> { uploadId }`
- `PATCH /api/upload/chunk?uploadId=...` + `X-Chunk-Index` + binary stream
- `POST /api/upload/complete` `{ uploadId }`
- `POST /api/upload/abort` `{ uploadId }`

Default chunk size: 16MB; temp chunk dir: `.axo/temp` (same level as storage by default).

### Auth

- `POST /api/auth/login` `{ username, password }`
- `POST /api/auth/logout`

## WebDAV

Mount at: `https://<host>:<https-port>/webdav/`. Shares the storage root with `AXO_STORAGE_DIR`.

## Docker

```bash
./scripts/build_docker.sh
```

Or specify image name/tag:

```bash
./scripts/build_docker.sh axo-drive v0.0.1
```

### Docker Compose example

```yaml
services:
  axo-drive:
    image: sfwwslm/axo-drive:0.0.1
    container_name: axo-drive
    ports:
      - "5005:5005"
      - "5006:5006"
    environment:
      AXO_STORAGE_DIR: /app/.axo/storage
      AXO_AUTH_USER: axo
      AXO_AUTH_PASS: axo
      AXO_HTTP_PORT: 5005
      AXO_HTTPS_PORT: 5006
    volumes:
      - ./data:/app/.axo/storage
    restart: unless-stopped
```

The build script reads the version from `Cargo.toml` as the image tag by default, or you can pass one explicitly:

```bash
./scripts/build_docker.sh axo-drive v0.0.1
```
