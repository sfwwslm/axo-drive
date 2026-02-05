// 目录列表接口的文件/目录条目信息。
export type FileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string | null;
  etag?: string | null;
};

// 后端版本信息展示结构。
export type VersionInfo = {
  version: string;
  build_time: string;
  build_env: string;
};

// 语言区域枚举。
export type Locale = "zh" | "en";

// 上传冲突对话框的用户动作类型。
export type UploadConflictAction = "reload" | "overwrite" | "saveAs" | "cancel";

// 上传冲突对话框需要展示的上下文数据。
export type UploadConflictState = {
  file: File;
  targetPath: string;
  uploadId: string;
  existing?: FileEntry | null;
};
