// API 路由前缀与上传配置常量。
export const BASE_API = "/api/files";
export const UPLOAD_API = "/api/upload";
export const AUTH_API = "/api/auth";
export const VERSION_API = "/api/version";

// 分片上传与并发配置。
export const CHUNK_SIZE = 16 * 1024 * 1024;
export const CHUNK_CONCURRENCY = 3;
export const FILE_CONCURRENCY = 2;
