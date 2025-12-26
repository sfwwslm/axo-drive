# AxoDrive

轻量级、零运行时依赖的局域网文件管理服务，提供 Web UI + WebDAV 挂载能力。

[English](README-en.md)

![bg](https://raw.githubusercontent.com/sfwwslm/axo-drive/main/docs/login.png)
![bg](https://raw.githubusercontent.com/sfwwslm/axo-drive/main/docs/files.png)

## 功能概览

- Web UI：文件列表、上传/下载、删除、目录创建、进度展示
- WebDAV：挂载为网络磁盘（与 HTTP API 共用存储目录）
- 分片上传：`init -> chunk -> complete` 流程，支持并发与重试
- 断点下载：Range 请求 + If-Range 处理，流式返回
- 内置认证：Web UI 使用 Cookie 会话，WebDAV 支持 Basic Auth
- 自动语言：中文浏览器显示中文，其它语言显示英文
- 单二进制分发：`frontend/dist` 构建产物嵌入 Rust 二进制

## 目录结构

```text
axo-drive/
├── Cargo.toml
├── src/
│   └── main.rs
├── frontend/
│   ├── src/
│   ├── public/
│   └── dist/            # 构建后产物（被嵌入）
├── build.rs             # build metadata (shadow-rs)
├── Dockerfile
└── scripts/
    ├── build.sh
    └── build_docker.sh
```

## 快速开始

### 运行后端

```bash
cargo run
```

默认同时启动：

- HTTP: `http://0.0.0.0:5005`
- HTTPS: `https://0.0.0.0:5006`（未传证书时自动生成自签名证书）

### 前端开发

```bash
cd frontend && pnpm dev
```

### 构建发布

```bash
cd frontend && pnpm build
cargo build --release
```

跨平台构建脚本（Linux musl + Windows gnu）：

```bash
./scripts/build.sh
```

## 配置参数

支持命令行与环境变量（命令行优先）：

- `--storage-dir` / `AXO_STORAGE_DIR`：文件存储目录（默认 `.axo/storage`）
- `--auth-user` / `AXO_AUTH_USER`：认证用户名（默认 `axo`）
- `--auth-pass` / `AXO_AUTH_PASS`：认证密码（默认 `axo`）
- `--host` / `AXO_BIND`：监听地址（默认 `0.0.0.0`）
- `--http-port` / `AXO_HTTP_PORT`：HTTP 端口（默认 `5005`）
- `--https-port` / `AXO_HTTPS_PORT`：HTTPS 端口（默认 `5006`）
- `--tls-cert` / `AXO_TLS_CERT`：TLS 证书路径（PEM）
- `--tls-key` / `AXO_TLS_KEY`：TLS 私钥路径（PEM）
- `--session-ttl-secs` / `AXO_SESSION_TTL_SECS`：会话 TTL（默认 86400 秒）
- `--login-max-attempts` / `AXO_LOGIN_MAX_ATTEMPTS`：登录最大尝试次数（默认 5，0 表示关闭限制）
- `--login-window-secs` / `AXO_LOGIN_WINDOW_SECS`：登录限制窗口（默认 300 秒）
- `--login-lockout-secs` / `AXO_LOGIN_LOCKOUT_SECS`：锁定时长（默认 600 秒）
- `--upload-max-size` / `AXO_UPLOAD_MAX_SIZE`：上传文件大小上限（默认 100GiB，0 表示不限制）
- `--upload-max-chunks` / `AXO_UPLOAD_MAX_CHUNKS`：单次上传最大分片数（默认 8192，0 表示不限制）
- `--upload-max-concurrent` / `AXO_UPLOAD_MAX_CONCURRENT`：并发上传数量上限（默认 8，0 表示不限制）
- `--upload-temp-ttl-secs` / `AXO_UPLOAD_TEMP_TTL_SECS`：临时目录过期清理阈值（默认 86400 秒，0 表示不清理）
- `--cors-origins` / `AXO_CORS_ORIGINS`：允许的 CORS 来源（逗号分隔）

未设置 `AXO_CORS_ORIGINS` 时不会输出 CORS 相关响应头。若前端来自其他域名，可设置该参数。

### CLI 使用

```bash
cargo run -- --help
```

示例：

```bash
cargo run -- -b 0.0.0.0 -p 8080 -P 8443 -s /data/axo-drive --auth-user axo --auth-pass axo
```

## 安全说明

- WebDAV 仅允许 HTTPS，避免 Basic Auth 明文传输。
- Web UI Cookie 使用 HttpOnly，SameSite=Strict，并在 HTTPS 下标记 Secure。
- 会话有 TTL 并定期清理，避免内存无限增长。
- 登录有速率限制与锁定策略。
- 上传受大小、分片数与并发数限制。
- 过期上传临时目录会被定期清理。

## API 概览

### 文件操作

- `GET /api/files/list?path=`：列目录
- `GET /api/files/download?path=`：下载（支持 Range）
- `PUT /api/files/write?path=`：直接写入
- `DELETE /api/files/delete?path=`：删除文件或目录
- `POST /api/files/mkdir`：新建目录

### 分片上传

- `POST /api/upload/init` `{ name, totalSize } -> { uploadId }`
- `PATCH /api/upload/chunk?uploadId=...` + `X-Chunk-Index` + 二进制流
- `POST /api/upload/complete` `{ uploadId }`
- `POST /api/upload/abort` `{ uploadId }`

默认分片大小：16MB；临时分片目录：`.axo/temp`（默认与存储目录同级）。

### 认证

- `POST /api/auth/login` `{ username, password }`
- `POST /api/auth/logout`

## WebDAV

挂载地址：`https://<host>:<https-port>/webdav/`。存储目录与 HTTP API 共用 `AXO_STORAGE_DIR`。

## Docker

```bash
./scripts/build_docker.sh
```

或指定镜像名/标签：

```bash
./scripts/build_docker.sh axo-drive v0.0.1
```

### Docker Compose 示例

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

构建脚本默认会从 `Cargo.toml` 读取版本号作为镜像 tag，也支持显式传参：

```bash
./scripts/build_docker.sh axo-drive v0.0.1
```
