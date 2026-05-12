# 更新日志

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，并遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [0.1.1] - 2026-05-12

### 新增

- 新增 MIT License 文件。

### 变更

- 后端：WebDAV 认证中间件移除 HTTPS 强制要求，支持通过 HTTP 或 HTTPS 挂载 `/webdav/`。
- 文档：更新 README 中的 WebDAV 挂载说明与 Docker 镜像示例标签。
- 工程化：Docker 构建基础镜像升级至 Node 24、Rust 1.95 与 Debian Trixie。
- 工程化：升级 GitHub Actions、Docker、pnpm 与 artifact 相关工作流 action 版本。

## [0.1.0] - 2026-03-24

### 变更

- 前端：优化 `App` 组件类型安全与异步函数调用流程。
- 前端：调整表格表头样式，避免文本换行导致的显示问题。
- 工程化：更新 ESLint 配置并升级相关前端依赖。
- 工程化：Docker 发布工作流新增 latest 标签可选发布开关。

## [0.1.0-alpha.3] - 2026-02-12

### 修复

- 后端：修复 WebDAV 认证头中的转义字符问题。
- 前端：补充按钮样式定义。

## [0.1.0-alpha.2] - 2026-02-05

适配手机浏览器与图片预览体验。

### 新增

- 前端：图片文件缩略图预览，支持点击弹层放大与左右切换。
- 前端：视频文件支持点击弹层播放。
- 前端：删除操作增加确认弹窗。

### 变更

- 前端：移动端布局优化（按钮全宽、列精简、面板与弹窗适配）。
- 前端：优化页面结构与响应速度，提升可维护性。
- 前端：上传/新建/删除后列表即时更新，减少刷新与闪动。

### 修复

- 前端：修复登出后登录页Logo追踪鼠标失效的问题。

## [0.1.0-alpha.1] - 2026-01-22

并发控制与冲突提示功能落地。

### 新增

- 后端：路径锁与锁超时返回 409，写入与上传完成使用原子替换。
- 后端：ETag/If-Match/If-None-Match 条件写入支持，list/write/upload 返回 ETag。
- 后端：WebDAV 使用内存锁系统，支持锁超时与续租。
- 前端：上传完成支持冲突提示（412），提供重试/覆盖/另存为。

## [0.0.1] - 2025-12-26

首个版本发布，提供局域网文件管理能力、Web UI 与 WebDAV 访问。

### 新增

- Web UI：文件列表、上传/下载、删除、创建目录。
- 分片上传：支持并发、限制与临时分片清理。
- 下载流式返回：支持 Range/If-Range。
- 认证：Web UI 使用 Cookie 会话并带登录限流；WebDAV 使用 Basic Auth。
- WebDAV 挂载于 `/webdav/`，与 HTTP API 共用存储目录。
- 前端静态资源嵌入二进制，实现单文件分发。
- HTTP/HTTPS 双端口启动，缺省自签名证书。
- Dockerfile 与构建脚本支持发布打包。

[0.1.1]: https://github.com/sfwwslm/axo-drive/compare/0.1.0...0.1.1
[0.1.0]: https://github.com/sfwwslm/axo-drive/compare/0.1.0-alpha.3...0.1.0
[0.1.0-alpha.3]: https://github.com/sfwwslm/axo-drive/compare/0.1.0-alpha.2...0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/sfwwslm/axo-drive/compare/0.1.0-alpha.1...0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/sfwwslm/axo-drive/compare/0.0.1...0.1.0-alpha.1
[0.0.1]: https://github.com/sfwwslm/axo-drive/releases/tag/0.0.1
