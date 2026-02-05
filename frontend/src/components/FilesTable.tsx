import type { MessageKey } from "../i18n";
import type { FileEntry } from "../types";
import { formatBytes } from "../utils";

type FilesTableProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
  entries: FileEntry[];
  loading: boolean;
  downloadingPath: string | null;
  onDirectoryClick: (entry: FileEntry) => void;
  onDownloadClick: (entry: FileEntry) => void;
  onDelete: (entry: FileEntry) => void;
  onImagePreview: (entry: FileEntry) => void;
  onVideoPreview: (entry: FileEntry) => void;
  isImageFile: (name: string) => boolean;
  isVideoFile: (name: string) => boolean;
  buildPreviewUrl: (path: string) => string;
};

// 文件列表表格（含预览入口与操作按钮）。
const FilesTable = ({
  t,
  entries,
  loading,
  downloadingPath,
  onDirectoryClick,
  onDownloadClick,
  onDelete,
  onImagePreview,
  onVideoPreview,
  isImageFile,
  isVideoFile,
  buildPreviewUrl,
}: FilesTableProps) => (
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
                      onClick={() => onImagePreview(entry)}
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
                  {!entry.is_dir && isVideoFile(entry.name) && (
                    <button
                      type="button"
                      className="file-preview-btn"
                      onClick={() => onVideoPreview(entry)}
                      aria-label={`播放 ${entry.name}`}
                    >
                      <span className="video-preview">▶</span>
                    </button>
                  )}
                  <span className="tooltip-wrapper" data-tooltip={entry.name}>
                    <button
                      type="button"
                      className={entry.is_dir ? "entry-name dir" : "entry-name"}
                      onClick={() => onDirectoryClick(entry)}
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
                    onClick={() => onDownloadClick(entry)}
                  >
                    {downloadingPath === entry.path
                      ? t("cancelDownload")
                      : t("download")}
                  </button>
                )}
                <button type="button" onClick={() => onDelete(entry)}>
                  {t("delete")}
                </button>
              </td>
            </tr>
          ))
        )}
      </tbody>
    </table>
  </div>
);

export default FilesTable;
