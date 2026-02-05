import type { MessageKey } from "../i18n";
import type { UploadConflictAction, UploadConflictState } from "../types";
import { formatBytes } from "../utils";

type UploadConflictModalProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
  conflict: UploadConflictState;
  onResolve: (action: UploadConflictAction) => void;
};

// 上传冲突处理弹窗。
const UploadConflictModal = ({
  t,
  conflict,
  onResolve,
}: UploadConflictModalProps) => (
  <div className="modal-backdrop" onClick={() => onResolve("cancel")}>
    <div className="modal" onClick={(event) => event.stopPropagation()}>
      <h3 className="modal-title">{t("uploadConflictTitle")}</h3>
      <p className="modal-body">{t("uploadConflictMessage")}</p>
      {conflict.existing && (
        <div className="conflict-meta">
          <span>{conflict.existing.name}</span>
          <span>
            {conflict.existing.modified ?? "—"} ·{" "}
            {formatBytes(conflict.existing.size)}
          </span>
          {conflict.existing.etag && (
            <span className="etag">ETag: {conflict.existing.etag}</span>
          )}
        </div>
      )}
      <div className="modal-actions">
        <button
          type="button"
          className="modal-cancel"
          onClick={() => onResolve("cancel")}
        >
          {t("uploadConflictCancel")}
        </button>
        <button
          type="button"
          className="modal-cancel"
          onClick={() => onResolve("reload")}
        >
          {t("uploadConflictReload")}
        </button>
        <button
          type="button"
          className="modal-cancel"
          onClick={() => onResolve("saveAs")}
        >
          {t("uploadConflictSaveAs")}
        </button>
        <button
          type="button"
          className="modal-submit"
          onClick={() => onResolve("overwrite")}
        >
          {t("uploadConflictOverwrite")}
        </button>
      </div>
    </div>
  </div>
);

export default UploadConflictModal;
