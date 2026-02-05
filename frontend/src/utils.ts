import axios from "axios";
import { BASE_API } from "./constants";

// 常用工具函数：路径拼接、格式化、类型判断与请求辅助。
export const joinPath = (base: string, segment: string) => {
  if (!segment) return base;
  return base ? `${base}/${segment}` : segment;
};

// 将字节数格式化为可读单位。
export const formatBytes = (value: number) => {
  if (value === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.floor(Math.log(value) / Math.log(1024));
  const number = value / Math.pow(1024, index);
  return `${number.toFixed(1)} ${units[index]}`;
};

// 统一格式化 UTC 时间（YYYY-MM-DD HH:mm:ss）。
export const formatUtcTimestamp = (date: Date) => {
  const pad = (value: number) => value.toString().padStart(2, "0");
  const year = date.getUTCFullYear();
  const month = pad(date.getUTCMonth() + 1);
  const day = pad(date.getUTCDate());
  const hours = pad(date.getUTCHours());
  const minutes = pad(date.getUTCMinutes());
  const seconds = pad(date.getUTCSeconds());
  return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`;
};

// 支持的图片扩展名集合。
const imageExtensions = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "svg",
  "avif",
]);

// 支持的视频扩展名集合。
const videoExtensions = new Set(["mp4", "webm", "ogg", "mov", "m4v"]);

// 判断文件名是否为图片类型。
export const isImageFile = (name: string) => {
  const dotIndex = name.lastIndexOf(".");
  if (dotIndex === -1) return false;
  const ext = name.slice(dotIndex + 1).toLowerCase();
  return imageExtensions.has(ext);
};

// 判断文件名是否为视频类型。
export const isVideoFile = (name: string) => {
  const dotIndex = name.lastIndexOf(".");
  if (dotIndex === -1) return false;
  const ext = name.slice(dotIndex + 1).toLowerCase();
  return videoExtensions.has(ext);
};

// 构建文件预览/下载 URL。
export const buildPreviewUrl = (path: string) =>
  `${BASE_API}/download?path=${encodeURIComponent(path)}`;

// 判断请求是否被取消。
export const isAbortError = (error: unknown) => {
  if (error instanceof DOMException && error.name === "AbortError") {
    return true;
  }
  if (axios.isAxiosError(error) && error.code === "ERR_CANCELED") {
    return true;
  }
  return false;
};
