import type { ChangeEvent, FormEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import axios from "axios";
import "./App.css";
import AuthCheckingScreen from "./components/AuthCheckingScreen";
import CreateFolderModal from "./components/CreateFolderModal";
import FilesTable from "./components/FilesTable";
import ImagePreviewModal from "./components/ImagePreviewModal";
import LoginScreen from "./components/LoginScreen";
import UploadConflictModal from "./components/UploadConflictModal";
import VideoPreviewModal from "./components/VideoPreviewModal";
import {
  AUTH_API,
  BASE_API,
  CHUNK_CONCURRENCY,
  CHUNK_SIZE,
  FILE_CONCURRENCY,
  UPLOAD_API,
  VERSION_API,
} from "./constants";
import type { MessageKey } from "./i18n";
import { formatMessage, resolveLocale, translations } from "./i18n";
import type {
  FileEntry,
  Locale,
  UploadConflictAction,
  UploadConflictState,
  VersionInfo,
} from "./types";
import {
  buildPreviewUrl,
  isAbortError,
  isImageFile,
  isVideoFile,
  joinPath,
} from "./utils";

axios.defaults.withCredentials = true;

function App() {
  const [locale] = useState<Locale>(resolveLocale);
  const t = useCallback(
    (key: MessageKey, vars?: Record<string, string | number>) =>
      formatMessage(translations[locale][key], vars),
    [locale],
  );
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [currentPath, setCurrentPath] = useState("");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [folderName, setFolderName] = useState("");
  const [createFolderOpen, setCreateFolderOpen] = useState(false);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const [previewVideo, setPreviewVideo] = useState<{
    src: string;
    name: string;
  } | null>(null);
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

  const fetchEntries = useCallback(
    async (path: string) => {
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
    },
    [t],
  );

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
  }, [currentPath, authRequired, authChecked, fetchEntries]);

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

  const handleImagePreview = (entry: FileEntry) => {
    const idx = imageEntries.findIndex((item) => item.path === entry.path);
    if (idx >= 0) {
      setPreviewVideo(null);
      setPreviewIndex(idx);
    }
  };

  const handleVideoPreview = (entry: FileEntry) => {
    setPreviewIndex(null);
    setPreviewVideo({
      src: buildPreviewUrl(entry.path),
      name: entry.name,
    });
  };

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
      setLoginFocus(false);
    }
  };

  useEffect(() => {
    if (authRequired) {
      setLoginFocus(false);
    }
  }, [authRequired]);

  if (!authChecked) {
    return <AuthCheckingScreen t={t} />;
  }

  if (authRequired) {
    return (
      <LoginScreen
        t={t}
        username={loginUsername}
        password={loginPassword}
        loggingIn={loggingIn}
        loginError={loginError}
        loginFocus={loginFocus}
        onUsernameChange={setLoginUsername}
        onPasswordChange={setLoginPassword}
        onLoginFocus={setLoginFocus}
        onSubmit={handleLogin}
      />
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

        <FilesTable
          t={t}
          entries={entries}
          loading={loading}
          downloadingPath={downloadingPath}
          onDirectoryClick={handleDirectoryClick}
          onDownloadClick={handleDownloadClick}
          onDelete={handleDelete}
          onImagePreview={handleImagePreview}
          onVideoPreview={handleVideoPreview}
          isImageFile={isImageFile}
          isVideoFile={isVideoFile}
          buildPreviewUrl={buildPreviewUrl}
        />
      </section>

      {createFolderOpen && (
        <CreateFolderModal
          t={t}
          folderName={folderName}
          onFolderNameChange={setFolderName}
          onClose={() => setCreateFolderOpen(false)}
          onSubmit={handleCreateFolder}
        />
      )}

      {previewEntry && (
        <ImagePreviewModal
          entries={imageEntries}
          previewIndex={previewIndex ?? 0}
          onClose={closePreview}
          onPrev={goPrevPreview}
          onNext={goNextPreview}
        />
      )}

      {previewVideo && (
        <VideoPreviewModal
          name={previewVideo.name}
          src={previewVideo.src}
          onClose={() => setPreviewVideo(null)}
        />
      )}

      {uploadConflict && (
        <UploadConflictModal
          t={t}
          conflict={uploadConflict}
          onResolve={resolveConflict}
        />
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
