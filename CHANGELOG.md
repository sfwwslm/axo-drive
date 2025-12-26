# 更新日志

## 0.0.1

首个版本发布，提供局域网文件管理能力、Web UI 与 WebDAV 访问。

- Web UI：文件列表、上传/下载、删除、创建目录。
- 分片上传：支持并发、限制与临时分片清理。
- 下载流式返回：支持 Range/If-Range。
- 认证：Web UI 使用 Cookie 会话并带登录限流；WebDAV 使用 Basic Auth。
- WebDAV 挂载于 `/webdav/`，与 HTTP API 共用存储目录。
- 前端静态资源嵌入二进制，实现单文件分发。
- HTTP/HTTPS 双端口启动，缺省自签名证书。
- Dockerfile 与构建脚本支持发布打包。
