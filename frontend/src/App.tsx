import type { ChangeEvent, FormEvent } from "react";
import { useEffect, useRef, useState } from "react";
import axios from "axios";
import "./App.css";

type FileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string | null;
  etag?: string | null;
};

type VersionInfo = {
  version: string;
  build_time: string;
  build_env: string;
};

const BASE_API = "/api/files";
const UPLOAD_API = "/api/upload";
const AUTH_API = "/api/auth";
const VERSION_API = "/api/version";

axios.defaults.withCredentials = true;

const joinPath = (base: string, segment: string) => {
  if (!segment) return base;
  return base ? `${base}/${segment}` : segment;
};

const formatBytes = (value: number) => {
  if (value === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.floor(Math.log(value) / Math.log(1024));
  const number = value / Math.pow(1024, index);
  return `${number.toFixed(1)} ${units[index]}`;
};

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

const isImageFile = (name: string) => {
  const dotIndex = name.lastIndexOf(".");
  if (dotIndex === -1) return false;
  const ext = name.slice(dotIndex + 1).toLowerCase();
  return imageExtensions.has(ext);
};

const buildPreviewUrl = (path: string) =>
  `${BASE_API}/download?path=${encodeURIComponent(path)}`;

const CHUNK_SIZE = 16 * 1024 * 1024;
const CHUNK_CONCURRENCY = 3;
const FILE_CONCURRENCY = 2;

type Locale = "zh" | "en";

const resolveLocale = (): Locale => {
  if (typeof navigator === "undefined") return "en";
  const language = navigator.language?.toLowerCase() ?? "en";
  return language.startsWith("zh") ? "zh" : "en";
};

const translations = {
  zh: {
    loadDirFailedWithStatus: "无法加载目录: {status}",
    readDirFailed: "读取目录失败",
    uploadCancelled: "上传已取消",
    initUploadFailed: "初始化上传失败",
    uploading: "上传中",
    uploadingEllipsis: "上传中…",
    verifying: "校验中",
    downloadComplete: "下载完成",
    downloadCancelled: "下载已取消",
    downloadFailed: "下载失败",
    uploadConflictTitle: "文件冲突",
    uploadConflictMessage: "该文件已在你上传期间被其他人修改。请选择处理方式。",
    uploadConflictReload: "重新加载并重试",
    uploadConflictOverwrite: "强制覆盖",
    uploadConflictSaveAs: "另存为新文件",
    uploadConflictCancel: "取消上传",
    enterFolderName: "请输入文件夹名称",
    createFolderFailed: "创建目录失败",
    createFolderSuccess: "目录创建成功",
    waiting: "等待中",
    completed: "已完成",
    uploadFailed: "上传失败",
    deleteFailed: "删除失败",
    deleteSuccess: "删除成功",
    invalidCredentials: "账号或密码错误",
    loginFailed: "登录失败",
    logoutFailed: "登出失败",
    tagline: "轻量、安全的局域网文件管理",
    checkingAuth: "正在检查登录状态…",
    username: "用户名",
    password: "密码",
    loggingIn: "登录中…",
    login: "登录",
    loggingOut: "正在退出…",
    logout: "退出登录",
    uploadFile: "上传文件",
    cancelUpload: "取消上传",
    createFolder: "新建目录",
    nameHeader: "名称",
    typeHeader: "类型",
    sizeHeader: "大小",
    modifiedHeader: "修改时间",
    actionsHeader: "操作",
    loadingDir: "目录加载中…",
    emptyDir: "空目录",
    cancelDownload: "取消下载",
    download: "下载",
    delete: "删除",
    folderNamePlaceholder: "输入目录名称",
    cancel: "取消",
    create: "创建",
    entryCount: "{files} 文件 · {dirs} 文件夹",
  },
  en: {
    loadDirFailedWithStatus: "Failed to load directory: {status}",
    readDirFailed: "Failed to load directory",
    uploadCancelled: "Upload cancelled",
    initUploadFailed: "Failed to initialize upload",
    uploading: "Uploading",
    uploadingEllipsis: "Uploading…",
    verifying: "Verifying",
    downloadComplete: "Download complete",
    downloadCancelled: "Download cancelled",
    downloadFailed: "Download failed",
    uploadConflictTitle: "File conflict",
    uploadConflictMessage:
      "This file was modified while you were uploading. Choose how to proceed.",
    uploadConflictReload: "Reload and retry",
    uploadConflictOverwrite: "Overwrite",
    uploadConflictSaveAs: "Save as copy",
    uploadConflictCancel: "Cancel upload",
    enterFolderName: "Enter a folder name",
    createFolderFailed: "Failed to create folder",
    createFolderSuccess: "Folder created",
    waiting: "Waiting",
    completed: "Completed",
    uploadFailed: "Upload failed",
    deleteFailed: "Failed to delete",
    deleteSuccess: "Deleted",
    invalidCredentials: "Invalid username or password",
    loginFailed: "Login failed",
    logoutFailed: "Logout failed",
    tagline: "Lightweight, secure LAN file manager",
    checkingAuth: "Checking login status…",
    username: "Username",
    password: "Password",
    loggingIn: "Signing in…",
    login: "Sign in",
    loggingOut: "Signing out…",
    logout: "Sign out",
    uploadFile: "Upload files",
    cancelUpload: "Cancel upload",
    createFolder: "Create folder",
    nameHeader: "Name",
    typeHeader: "Type",
    sizeHeader: "Size",
    modifiedHeader: "Modified",
    actionsHeader: "Actions",
    loadingDir: "Loading directory…",
    emptyDir: "Empty directory",
    cancelDownload: "Cancel download",
    download: "Download",
    delete: "Delete",
    folderNamePlaceholder: "Folder name",
    cancel: "Cancel",
    create: "Create",
    entryCount: "{files} files · {dirs} folders",
  },
} as const;

type MessageKey = keyof typeof translations.en;

const formatMessage = (
  template: string,
  vars?: Record<string, string | number>,
) => {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, key) => String(vars[key] ?? ""));
};

const isAbortError = (error: unknown) => {
  if (error instanceof DOMException && error.name === "AbortError") {
    return true;
  }
  if (axios.isAxiosError(error) && error.code === "ERR_CANCELED") {
    return true;
  }
  return false;
};

type UploadConflictAction = "reload" | "overwrite" | "saveAs" | "cancel";

type UploadConflictState = {
  file: File;
  targetPath: string;
  uploadId: string;
  existing?: FileEntry | null;
};

type LoginLogoProps = {
  className?: string;
  sleep?: boolean;
};

const LoginLogo = ({ className, sleep = false }: LoginLogoProps) => {
  const svgRef = useRef<SVGSVGElement | null>(null);

  useEffect(() => {
    const handleMove = (event: MouseEvent) => {
      const svg = svgRef.current;
      if (!svg) return;
      if (svg.getAttribute("data-sleep") === "true") return;
      const rect = svg.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      const centerX = rect.left + rect.width / 2;
      const centerY = rect.top + rect.height / 2;
      const dx = event.clientX - centerX;
      const dy = event.clientY - centerY;
      const max = Math.min(rect.width, rect.height) / 2;
      const nx = Math.max(-1, Math.min(1, dx / max));
      const ny = Math.max(-1, Math.min(1, dy / max));
      const offset = 3.5;
      svg.style.setProperty("--eye-x", `${nx * offset}px`);
      svg.style.setProperty("--eye-y", `${ny * offset}px`);
    };

    window.addEventListener("mousemove", handleMove);
    return () => window.removeEventListener("mousemove", handleMove);
  }, []);

  return (
    <svg
      ref={svgRef}
      className={className}
      data-sleep={sleep ? "true" : "false"}
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 256 256"
      role="img"
      aria-label="AxoDrive"
    >
      <path
        d="M64 92C64 71.908 80.908 55 101 55H155C175.092 55 192 71.908 192 92V160C192 180.092 175.092 197 155 197H101C80.908 197 64 180.092 64 160V92Z"
        fill="#D4E5F6"
      />
      <path
        d="M128 74C160.078 74 186 99.922 186 132C186 164.078 160.078 190 128 190C95.922 190 70 164.078 70 132C70 99.922 95.922 74 128 74Z"
        fill="#F7FBFF"
      />
      <path
        d="M84 108L56 88"
        stroke="#3B82F6"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <path
        d="M172 108L200 88"
        stroke="#3B82F6"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <path
        d="M96 150C96 150 112 162 128 162C144 162 160 150 160 150"
        fill="none"
        stroke="#1D4ED8"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <circle className="eye" cx="104" cy="124" r="8" fill="#1E3A8A" />
      <circle className="eye" cx="152" cy="124" r="8" fill="#1E3A8A" />
    </svg>
  );
};

function App() {
  const [locale] = useState<Locale>(resolveLocale);
  const t = (key: MessageKey, vars?: Record<string, string | number>) =>
    formatMessage(translations[locale][key], vars);
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [currentPath, setCurrentPath] = useState("");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [folderName, setFolderName] = useState("");
  const [createFolderOpen, setCreateFolderOpen] = useState(false);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const previewTouchStartX = useRef<number | null>(null);
  const [uploading, setUploading] = useState(false);
  const [uploadProgress, setUploadProgress] = useState<number | null>(null);
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number | null>(null);
  const [authRequired, setAuthRequired] = useState(true);
  const [authChecked, setAuthChecked] = useState(false);
  const [loginUsername, setLoginUsername] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [loginError, setLoginError] = useState<string | null>(null);
  const [loggingIn, setLoggingIn] = useState(false);
  const [loggingOut, setLoggingOut] = useState(false);
  const [loginFocus, setLoginFocus] = useState(false);
  const [versionInfo, setVersionInfo] = useState<VersionInfo | null>(null);
  const [uploadConflict, setUploadConflict] =
    useState<UploadConflictState | null>(null);

  const uploadControllersRef = useRef<AbortController[]>([]);
  const uploadCancelledRef = useRef(false);
  const downloadControllerRef = useRef<AbortController | null>(null);
  const uploadIdsRef = useRef<Set<string>>(new Set());
  const downloadTokenRef = useRef(0);
  const conflictResolverRef = useRef<
    ((action: UploadConflictAction) => void) | null
  >(null);

  const listUrl = (path: string) => {
    const params = new URLSearchParams();
    if (path) {
      params.set("path", path);
    }
    const query = params.toString();
    return `${BASE_API}/list${query ? `?${query}` : ""}`;
  };

  const fetchEntries = async (path = currentPath) => {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(listUrl(path));
      if (response.status === 401) {
        setAuthRequired(true);
        return null;
      }
      if (!response.ok) {
        throw new Error(
          t("loadDirFailedWithStatus", { status: response.status }),
        );
      }
      const data: FileEntry[] = await response.json();
      setEntries(data);
      return data;
    } catch (err) {
      setError(err instanceof Error ? err.message : t("readDirFailed"));
      return null;
    } finally {
      setLoading(false);
    }
  };

  const cancelUpload = () => {
    if (!uploading) return;
    uploadCancelledRef.current = true;
    uploadControllersRef.current.forEach((controller) => controller.abort());
    uploadControllersRef.current = [];
    setUploading(false);
    setUploadProgress(null);
    setStatus(t("uploadCancelled"));
    if (uploadConflict) {
      resolveConflict("cancel");
    }
    void abortIncompleteUploads();
  };

  const cancelDownload = () => {
    if (!downloadingPath) return;
    downloadControllerRef.current?.abort();
    downloadControllerRef.current = null;
    setDownloadingPath(null);
    setDownloadProgress(null);
  };

  const abortIncompleteUploads = async () => {
    const ids = Array.from(uploadIdsRef.current);
    if (!ids.length) return;
    uploadIdsRef.current.clear();
    await Promise.all(
      ids.map((uploadId) =>
        axios.post(`${UPLOAD_API}/abort`, { uploadId }).catch(() => undefined),
      ),
    );
  };

  useEffect(() => {
    let active = true;
    const checkAuth = async () => {
      try {
        await axios.get(`${AUTH_API}/status`);
        if (!active) return;
        setAuthRequired(false);
      } catch (err) {
        if (!active) return;
        if (axios.isAxiosError(err) && err.response?.status === 401) {
          setAuthRequired(true);
        }
      } finally {
        if (active) {
          setAuthChecked(true);
        }
      }
    };
    checkAuth();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!authChecked || authRequired) {
      setVersionInfo(null);
      return;
    }
    let active = true;
    const fetchVersion = async () => {
      try {
        const response = await axios.get(VERSION_API);
        const data = response.data;
        if (active && data?.version) {
          setVersionInfo(data as VersionInfo);
        }
      } catch {
        if (active) {
          setVersionInfo(null);
        }
      }
    };
    fetchVersion();
    return () => {
      active = false;
    };
  }, [authChecked, authRequired]);

  useEffect(() => {
    if (authRequired || !authChecked) return;
    fetchEntries(currentPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentPath, authRequired, authChecked]);

  const handleAuthError = (err: unknown) => {
    if (axios.isAxiosError(err) && err.response?.status === 401) {
      setAuthRequired(true);
      setAuthChecked(true);
      return true;
    }
    return false;
  };

  const abortUpload = async (uploadId: string) => {
    await axios
      .post(`${UPLOAD_API}/abort`, { uploadId })
      .catch(() => undefined);
  };

  const resolveConflict = (action: UploadConflictAction) => {
    conflictResolverRef.current?.(action);
    conflictResolverRef.current = null;
    setUploadConflict(null);
  };

  const awaitConflictResolution = (state: UploadConflictState) =>
    new Promise<UploadConflictAction>((resolve) => {
      conflictResolverRef.current = resolve;
      setUploadConflict(state);
    });

  const buildCopyPath = (path: string) => {
    const parts = path.split("/");
    const name = parts.pop() ?? path;
    const dot = name.lastIndexOf(".");
    const base = dot > 0 ? name.slice(0, dot) : name;
    const ext = dot > 0 ? name.slice(dot) : "";
    const timestamp = new Date()
      .toISOString()
      .replace(/[-:]/g, "")
      .replace("T", "-")
      .slice(0, 15);
    const copyName = `${base} (copy ${timestamp})${ext}`;
    return [...parts, copyName].filter(Boolean).join("/");
  };

  const completeUpload = async (
    uploadId: string,
    headers?: Record<string, string>,
  ) => {
    await axios.post(
      `${UPLOAD_API}/complete`,
      { uploadId },
      headers ? { headers } : undefined,
    );
  };

  const uploadFileInChunks = async (
    file: File,
    targetPath: string,
    onChunk: (chunkBytes: number) => void,
  ) => {
    const initResponse = await axios.post(`${UPLOAD_API}/init`, {
      name: targetPath,
      totalSize: file.size,
    });
    const uploadId = initResponse.data?.uploadId as string | undefined;
    if (!uploadId) {
      throw new Error(t("initUploadFailed"));
    }
    uploadIdsRef.current.add(uploadId);
    setStatus(t("uploading"));

    const chunkCount = Math.max(1, Math.ceil(file.size / CHUNK_SIZE));
    let nextChunkIndex = 0;
    const chunkProgress = new Map<number, number>();

    const runChunkUpload = async (chunkIndex: number) => {
      const start = chunkIndex * CHUNK_SIZE;
      const end = Math.min(file.size, start + CHUNK_SIZE);
      const chunk = file.slice(start, end);
      const controller = new AbortController();
      uploadControllersRef.current.push(controller);
      let lastLoaded = 0;

      const cleanup = () => {
        uploadControllersRef.current = uploadControllersRef.current.filter(
          (item) => item !== controller,
        );
      };

      try {
        await axios.patch(`${UPLOAD_API}/chunk`, chunk, {
          params: { uploadId },
          headers: {
            "Content-Type": "application/octet-stream",
            "X-Chunk-Index": chunkIndex.toString(),
          },
          signal: controller.signal,
          onUploadProgress: (event) => {
            const loaded = event.loaded ?? 0;
            const delta = loaded - lastLoaded;
            if (delta > 0) {
              lastLoaded = loaded;
              const previous = chunkProgress.get(chunkIndex) ?? 0;
              chunkProgress.set(chunkIndex, previous + delta);
              onChunk(delta);
            }
          },
        });
        const remaining = chunk.size - lastLoaded;
        if (remaining > 0) {
          const previous = chunkProgress.get(chunkIndex) ?? 0;
          chunkProgress.set(chunkIndex, previous + remaining);
          onChunk(remaining);
        }
      } finally {
        cleanup();
      }
    };

    const runChunkUploadWithRetry = async (chunkIndex: number) => {
      let attempts = 0;
      while (true) {
        if (uploadCancelledRef.current) {
          throw new DOMException("Aborted", "AbortError");
        }
        if (attempts > 0) {
          const previous = chunkProgress.get(chunkIndex) ?? 0;
          if (previous > 0) {
            chunkProgress.set(chunkIndex, 0);
            onChunk(-previous);
          }
        }
        try {
          await runChunkUpload(chunkIndex);
          return;
        } catch (err) {
          if (isAbortError(err)) {
            throw err;
          }
          attempts += 1;
          if (attempts >= 3) {
            throw err;
          }
        }
      }
    };

    const worker = async () => {
      while (true) {
        if (uploadCancelledRef.current) {
          throw new DOMException("Aborted", "AbortError");
        }
        const chunkIndex = nextChunkIndex++;
        if (chunkIndex >= chunkCount) {
          return;
        }
        await runChunkUploadWithRetry(chunkIndex);
      }
    };

    const workers = Array.from(
      { length: Math.min(chunkCount, CHUNK_CONCURRENCY) },
      () => worker(),
    );

    let completed = false;
    try {
      await Promise.all(workers);

      setStatus(t("verifying"));
      let headers: Record<string, string> | undefined;
      const existing = entries.find((entry) => entry.path === targetPath);
      if (existing?.etag) {
        headers = { "If-Match": existing.etag };
      } else if (!existing) {
        headers = { "If-None-Match": "*" };
      }

      while (true) {
        try {
          await completeUpload(uploadId, headers);
          completed = true;
          break;
        } catch (err) {
          if (axios.isAxiosError(err) && err.response?.status === 412) {
            const conflictEntry =
              entries.find((entry) => entry.path === targetPath) ?? null;
            const action = await awaitConflictResolution({
              file,
              targetPath,
              uploadId,
              existing: conflictEntry,
            });
            if (action === "overwrite") {
              headers = undefined;
              continue;
            }
            if (action === "reload") {
              const data = await fetchEntries(currentPath);
              const refreshed = data?.find(
                (entry) => entry.path === targetPath,
              );
              if (refreshed?.etag) {
                headers = { "If-Match": refreshed.etag };
              } else {
                headers = { "If-None-Match": "*" };
              }
              continue;
            }
            if (action === "saveAs") {
              const newPath = buildCopyPath(targetPath);
              await abortUpload(uploadId);
              uploadIdsRef.current.delete(uploadId);
              await uploadFileInChunks(file, newPath, onChunk);
              completed = true;
              break;
            }
            await abortUpload(uploadId);
            uploadIdsRef.current.delete(uploadId);
            throw new DOMException("Aborted", "AbortError");
          }
          throw err;
        }
      }
    } finally {
      uploadIdsRef.current.delete(uploadId);
      if (!completed) {
        await abortUpload(uploadId);
      }
    }
  };

  const handleDirectoryClick = (entry: FileEntry) => {
    if (!entry.is_dir) return;
    setCurrentPath(entry.path);
  };

  const breadcrumb = () => {
    const segments = currentPath ? currentPath.split("/") : [];
    const items: { label: string; path: string | null }[] = [
      { label: "🏠", path: "" },
    ];
    if (segments.length <= 5) {
      items.push(
        ...segments.map((segment, index) => ({
          label: segment,
          path: segments.slice(0, index + 1).join("/"),
        })),
      );
      return items;
    }
    items.push({
      label: segments[0],
      path: segments[0],
    });
    items.push({ label: "…", path: null });
    items.push({
      label: segments[segments.length - 2],
      path: segments.slice(0, segments.length - 1).join("/"),
    });
    items.push({
      label: segments[segments.length - 1],
      path: segments.join("/"),
    });
    return items;
  };

  const handleBreadcrumbClick = (path: string) => {
    setCurrentPath(path);
  };

  const handleDownloadClick = async (entry: FileEntry) => {
    if (downloadingPath === entry.path) {
      cancelDownload();
      return;
    }
    downloadControllerRef.current?.abort();
    const controller = new AbortController();
    downloadControllerRef.current = controller;
    const downloadToken = downloadTokenRef.current + 1;
    downloadTokenRef.current = downloadToken;
    setDownloadingPath(entry.path);
    setStatus(null);
    setError(null);
    setDownloadProgress(0);

    try {
      const response = await axios.get(`${BASE_API}/download`, {
        params: { path: entry.path },
        responseType: "blob",
        signal: controller.signal,
        onDownloadProgress: (event) => {
          if (controller.signal.aborted) return;
          if (downloadTokenRef.current !== downloadToken) return;
          if (!event.total) return;
          const percent = Math.min(
            100,
            Math.round((event.loaded / event.total) * 100),
          );
          setDownloadProgress(percent);
        },
      });
      const blob = response.data as Blob;
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = entry.name;
      link.click();
      URL.revokeObjectURL(url);
      setStatus(t("downloadComplete"));
    } catch (err) {
      if (isAbortError(err)) {
        setStatus(t("downloadCancelled"));
      } else if (handleAuthError(err)) {
        setStatus(null);
      } else {
        setError(err instanceof Error ? err.message : t("downloadFailed"));
      }
    } finally {
      setDownloadingPath(null);
      if (downloadControllerRef.current === controller) {
        downloadControllerRef.current = null;
      }
      setDownloadProgress(null);
    }
  };

  const imageEntries = entries.filter(
    (entry) => !entry.is_dir && isImageFile(entry.name),
  );

  const previewEntry =
    previewIndex !== null ? imageEntries[previewIndex] : null;

  const closePreview = () => setPreviewIndex(null);

  const goPrevPreview = () => {
    if (previewIndex === null || imageEntries.length === 0) return;
    setPreviewIndex(
      (previewIndex - 1 + imageEntries.length) % imageEntries.length,
    );
  };

  const goNextPreview = () => {
    if (previewIndex === null || imageEntries.length === 0) return;
    setPreviewIndex((previewIndex + 1) % imageEntries.length);
  };

  useEffect(() => {
    if (previewIndex === null) return;
    if (previewIndex < 0 || previewIndex >= imageEntries.length) {
      setPreviewIndex(null);
    }
  }, [previewIndex, imageEntries.length]);

  const handleCreateFolder = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = folderName.trim();
    if (!trimmed) {
      setStatus(t("enterFolderName"));
      return;
    }

    const path = joinPath(currentPath, trimmed);
    setStatus(null);
    setError(null);
    try {
      const response = await fetch(`${BASE_API}/mkdir`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path }),
      });
      if (response.status === 401) {
        setAuthRequired(true);
        return;
      }
      if (!response.ok) {
        throw new Error(t("createFolderFailed"));
      }
      setFolderName("");
      setCreateFolderOpen(false);
      setStatus(t("createFolderSuccess"));
      fetchEntries(currentPath);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("createFolderFailed"));
    }
  };

  const handleUpload = async (event: ChangeEvent<HTMLInputElement>) => {
    const inputElement = event.currentTarget;
    const files = Array.from(inputElement.files ?? []);
    if (!files.length) return;

    const totalBytes = files.reduce((sum, file) => sum + file.size, 0);
    let uploadedBytes = 0;

    setUploading(true);
    uploadCancelledRef.current = false;
    setStatus(null);
    setError(null);
    setUploadProgress(totalBytes > 0 ? 0 : null);
    setStatus(t("waiting"));

    const handleChunkProgress = (chunkBytes: number) => {
      uploadedBytes = Math.max(0, uploadedBytes + chunkBytes);
      if (totalBytes > 0) {
        setUploadProgress(
          Math.min(100, Math.round((uploadedBytes / totalBytes) * 100)),
        );
      }
    };

    const uploadFileWithTracking = async (file: File) => {
      const targetPath = joinPath(currentPath, file.name);
      await uploadFileInChunks(file, targetPath, handleChunkProgress);
    };

    try {
      let nextFileIndex = 0;
      const fileWorkers = Array.from(
        { length: Math.min(files.length, FILE_CONCURRENCY) },
        async () => {
          while (nextFileIndex < files.length) {
            const fileIndex = nextFileIndex;
            nextFileIndex += 1;
            const file = files[fileIndex];
            await uploadFileWithTracking(file);
          }
        },
      );
      await Promise.all(fileWorkers);
      if (totalBytes > 0) {
        setUploadProgress(100);
      }
      setStatus(t("completed"));
      fetchEntries(currentPath);
    } catch (err) {
      if (isAbortError(err)) {
        setStatus(t("uploadCancelled"));
      } else if (handleAuthError(err)) {
        setStatus(null);
      } else {
        setError(err instanceof Error ? err.message : t("uploadFailed"));
      }
    } finally {
      setUploading(false);
      setUploadProgress(null);
      inputElement.value = "";
      uploadControllersRef.current = [];
    }
  };

  const handleDelete = async (entry: FileEntry) => {
    setStatus(null);
    setError(null);
    try {
      const response = await fetch(
        `${BASE_API}/delete?path=${encodeURIComponent(entry.path)}`,
        {
          method: "DELETE",
        },
      );
      if (response.status === 401) {
        setAuthRequired(true);
        return;
      }
      if (!response.ok) {
        throw new Error(t("deleteFailed"));
      }
      setStatus(t("deleteSuccess"));
      fetchEntries(currentPath);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("deleteFailed"));
    }
  };

  const handleLogin = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setLoggingIn(true);
    setLoginError(null);
    try {
      await axios.post(`${AUTH_API}/login`, {
        username: loginUsername,
        password: loginPassword,
      });
      setAuthRequired(false);
      setLoginPassword("");
      fetchEntries(currentPath);
    } catch (err) {
      if (axios.isAxiosError(err) && err.response?.status === 401) {
        setLoginError(t("invalidCredentials"));
      } else {
        setLoginError(t("loginFailed"));
      }
    } finally {
      setLoggingIn(false);
    }
  };

  const handleLogout = async () => {
    setLoggingOut(true);
    try {
      await axios.post(`${AUTH_API}/logout`);
    } catch (err) {
      if (!handleAuthError(err)) {
        setError(t("logoutFailed"));
      }
    } finally {
      setLoggingOut(false);
      setAuthRequired(true);
      setEntries([]);
    }
  };

  if (!authChecked) {
    return (
      <div className="app-shell login-shell">
        <div className="login-hero">
          <div className="login-brand">
            <span className="brand-title">AxoDrive</span>
            <p className="login-tagline">{t("tagline")}</p>
          </div>
          <section className="panel login-panel">
            <div className="login-logo-wrap">
              <LoginLogo className="login-logo" />
            </div>
            <p className="login-loading">{t("checkingAuth")}</p>
          </section>
        </div>
      </div>
    );
  }

  if (authRequired) {
    return (
      <div className="app-shell login-shell">
        <div className="login-hero">
          <div className="login-brand">
            <span className="brand-title">AxoDrive</span>
            <p className="login-tagline">{t("tagline")}</p>
          </div>
          <section className="panel login-panel">
            <div className="login-logo-wrap">
              <LoginLogo className="login-logo" sleep={loginFocus} />
            </div>
            <form className="login-form" onSubmit={handleLogin}>
              <label>
                {t("username")}
                <input
                  value={loginUsername}
                  onChange={(event) => setLoginUsername(event.target.value)}
                  onFocus={() => setLoginFocus(true)}
                  onBlur={() => setLoginFocus(false)}
                  autoComplete="username"
                />
              </label>
              <label>
                {t("password")}
                <input
                  type="password"
                  value={loginPassword}
                  onChange={(event) => setLoginPassword(event.target.value)}
                  onFocus={() => setLoginFocus(true)}
                  onBlur={() => setLoginFocus(false)}
                  autoComplete="current-password"
                />
              </label>
              {loginError && <p className="status error">{loginError}</p>}
              <div className="login-actions">
                <button type="submit" disabled={loggingIn}>
                  {loggingIn ? t("loggingIn") : t("login")}
                </button>
              </div>
            </form>
          </section>
        </div>
      </div>
    );
  }

  const dirCount = entries.filter((entry) => entry.is_dir).length;
  const fileCount = entries.length - dirCount;
  const versionLabel = versionInfo
    ? versionInfo.version.startsWith("v")
      ? versionInfo.version
      : `v${versionInfo.version}`
    : null;

  return (
    <div className="app-shell">
      {(uploadProgress !== null || downloadProgress !== null) && (
        <div className="transfer-progress-track">
          {uploadProgress !== null && (
            <div
              className="transfer-progress-indicator upload"
              style={{ width: `${uploadProgress}%` }}
              role="progressbar"
              aria-valuenow={uploadProgress}
              aria-valuemin={0}
              aria-valuemax={100}
            />
          )}
          {downloadProgress !== null && (
            <div
              className="transfer-progress-indicator download"
              style={{ width: `${downloadProgress}%` }}
              role="progressbar"
              aria-valuenow={downloadProgress}
              aria-valuemin={0}
              aria-valuemax={100}
            />
          )}
        </div>
      )}
      <header>
        <span className="brand-title">AxoDrive</span>
        <button
          type="button"
          className="logout-btn"
          onClick={handleLogout}
          disabled={loggingOut}
        >
          {loggingOut ? t("loggingOut") : t("logout")}
        </button>
      </header>

      <section className="panel">
        <div className="panel-actions">
          <div className="breadcrumbs">
            {breadcrumb().map((item, index, items) => (
              <div
                className="breadcrumb-item"
                key={`${item.path ?? "ellipsis"}-${index}`}
              >
                {item.path === null ? (
                  <span className="crumb-ellipsis">{item.label}</span>
                ) : (
                  <button
                    type="button"
                    className="crumb"
                    onClick={() => handleBreadcrumbClick(item.path!)}
                  >
                    {item.label}
                  </button>
                )}
                {index < items.length - 1 && (
                  <span className="crumb-separator" aria-hidden="true">
                    /
                  </span>
                )}
              </div>
            ))}
          </div>
          <div className="controls">
            <label className="upload-btn">
              <input type="file" multiple onChange={handleUpload} />
              {uploading ? t("uploadingEllipsis") : t("uploadFile")}
            </label>
            {uploading && (
              <button
                type="button"
                className="cancel-btn"
                onClick={cancelUpload}
              >
                {t("cancelUpload")}
              </button>
            )}
            <button
              type="button"
              className="mkdir-trigger"
              onClick={() => {
                setFolderName("");
                setCreateFolderOpen(true);
              }}
            >
              {t("createFolder")}
            </button>
          </div>
        </div>

        {error && <p className="status error">{error}</p>}
        {status && !error && <p className="status success">{status}</p>}

        <div className="table-wrapper">
          <table>
            <thead>
              <tr>
                <th className="col-index">#</th>
                <th>{t("nameHeader")}</th>
                <th className="col-type">{t("typeHeader")}</th>
                <th className="col-size">{t("sizeHeader")}</th>
                <th className="col-modified">{t("modifiedHeader")}</th>
                <th>{t("actionsHeader")}</th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <tr>
                  <td colSpan={6} className="placeholder">
                    {t("loadingDir")}
                  </td>
                </tr>
              ) : entries.length === 0 ? (
                <tr>
                  <td colSpan={6} className="placeholder">
                    {t("emptyDir")}
                  </td>
                </tr>
              ) : (
                entries.map((entry, index) => (
                  <tr key={entry.path}>
                    <td className="col-index">{index + 1}</td>
                    <td className="name-cell">
                      <div className="name-cell-content">
                        {!entry.is_dir && isImageFile(entry.name) && (
                          <button
                            type="button"
                            className="file-preview-btn"
                            onClick={() => {
                              const idx = imageEntries.findIndex(
                                (item) => item.path === entry.path,
                              );
                              if (idx >= 0) setPreviewIndex(idx);
                            }}
                            aria-label={`预览 ${entry.name}`}
                          >
                            <img
                              className="file-preview"
                              src={buildPreviewUrl(entry.path)}
                              alt={entry.name}
                              loading="lazy"
                            />
                          </button>
                        )}
                        <span
                          className="tooltip-wrapper"
                          data-tooltip={entry.name}
                        >
                          <button
                            type="button"
                            className={
                              entry.is_dir ? "entry-name dir" : "entry-name"
                            }
                            onClick={() => handleDirectoryClick(entry)}
                          >
                            {entry.name}
                          </button>
                        </span>
                      </div>
                    </td>
                    <td className="col-type">{entry.is_dir ? "📁" : "📄"}</td>
                    <td className="col-size">
                      {entry.is_dir ? "—" : formatBytes(entry.size)}
                    </td>
                    <td className="col-modified">{entry.modified ?? "—"}</td>
                    <td className="actions">
                      {!entry.is_dir && (
                        <button
                          type="button"
                          className="link"
                          onClick={() => handleDownloadClick(entry)}
                        >
                          {downloadingPath === entry.path
                            ? t("cancelDownload")
                            : t("download")}
                        </button>
                      )}
                      <button type="button" onClick={() => handleDelete(entry)}>
                        {t("delete")}
                      </button>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </section>

      {createFolderOpen && (
        <div
          className="modal-backdrop"
          onClick={() => setCreateFolderOpen(false)}
        >
          <div className="modal" onClick={(event) => event.stopPropagation()}>
            <h3 className="modal-title">{t("createFolder")}</h3>
            <form className="modal-form" onSubmit={handleCreateFolder}>
              <input
                className="modal-input"
                placeholder={t("folderNamePlaceholder")}
                value={folderName}
                onChange={(event) => setFolderName(event.target.value)}
                autoFocus
              />
              <div className="modal-actions">
                <button
                  type="button"
                  className="modal-cancel"
                  onClick={() => setCreateFolderOpen(false)}
                >
                  {t("cancel")}
                </button>
                <button type="submit" className="modal-submit">
                  {t("create")}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {previewEntry && (
        <div className="modal-backdrop" onClick={closePreview}>
          <div
            className="modal image-modal"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="image-modal-header">
              <h3 className="modal-title">{previewEntry.name}</h3>
              <button
                type="button"
                className="image-modal-close"
                onClick={closePreview}
              >
                ×
              </button>
            </div>
            <div
              className="image-modal-body"
              onTouchStart={(event) => {
                previewTouchStartX.current = event.touches[0]?.clientX ?? null;
              }}
              onTouchEnd={(event) => {
                const startX = previewTouchStartX.current;
                if (startX === null) return;
                const endX = event.changedTouches[0]?.clientX ?? startX;
                const delta = endX - startX;
                previewTouchStartX.current = null;
                if (Math.abs(delta) < 40) return;
                if (delta > 0) {
                  goPrevPreview();
                } else {
                  goNextPreview();
                }
              }}
            >
              {imageEntries.length > 1 && (
                <>
                  <button
                    type="button"
                    className="image-nav-btn left"
                    onClick={goPrevPreview}
                    aria-label="上一张"
                  >
                    ‹
                  </button>
                  <button
                    type="button"
                    className="image-nav-btn right"
                    onClick={goNextPreview}
                    aria-label="下一张"
                  >
                    ›
                  </button>
                </>
              )}
              <img
                src={buildPreviewUrl(previewEntry.path)}
                alt={previewEntry.name}
              />
            </div>
          </div>
        </div>
      )}
      {uploadConflict && (
        <div
          className="modal-backdrop"
          onClick={() => resolveConflict("cancel")}
        >
          <div className="modal" onClick={(event) => event.stopPropagation()}>
            <h3 className="modal-title">{t("uploadConflictTitle")}</h3>
            <p className="modal-body">{t("uploadConflictMessage")}</p>
            {uploadConflict.existing && (
              <div className="conflict-meta">
                <span>{uploadConflict.existing.name}</span>
                <span>
                  {uploadConflict.existing.modified ?? "—"} ·{" "}
                  {formatBytes(uploadConflict.existing.size)}
                </span>
                {uploadConflict.existing.etag && (
                  <span className="etag">
                    ETag: {uploadConflict.existing.etag}
                  </span>
                )}
              </div>
            )}
            <div className="modal-actions">
              <button
                type="button"
                className="modal-cancel"
                onClick={() => resolveConflict("cancel")}
              >
                {t("uploadConflictCancel")}
              </button>
              <button
                type="button"
                className="modal-cancel"
                onClick={() => resolveConflict("reload")}
              >
                {t("uploadConflictReload")}
              </button>
              <button
                type="button"
                className="modal-cancel"
                onClick={() => resolveConflict("saveAs")}
              >
                {t("uploadConflictSaveAs")}
              </button>
              <button
                type="button"
                className="modal-submit"
                onClick={() => resolveConflict("overwrite")}
              >
                {t("uploadConflictOverwrite")}
              </button>
            </div>
          </div>
        </div>
      )}
      <footer className="footer">
        <div className="footer-content">
          {!loading && (
            <span className="entry-count">
              {t("entryCount", { files: fileCount, dirs: dirCount })}
            </span>
          )}
          {versionLabel && (
            <span className="version-watermark">{versionLabel}</span>
          )}
        </div>
      </footer>
    </div>
  );
}

export default App;
