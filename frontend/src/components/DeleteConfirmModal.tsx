import type { MessageKey } from "../i18n";
import type { FileEntry } from "../types";

type DeleteConfirmModalProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
  entry: FileEntry;
  onCancel: () => void;
  onConfirm: () => void;
};

// 删除确认弹窗。
const DeleteConfirmModal = ({
  t,
  entry,
  onCancel,
  onConfirm,
}: DeleteConfirmModalProps) => (
  <div className="modal-backdrop" onClick={onCancel}>
    <div className="modal" onClick={(event) => event.stopPropagation()}>
      <h3 className="modal-title">{t("deleteConfirmTitle")}</h3>
      <p className="modal-body">
        {t("deleteConfirmMessage", { name: entry.name })}
      </p>
      <div className="modal-actions">
        <button type="button" className="modal-cancel" onClick={onCancel}>
          {t("deleteConfirmCancel")}
        </button>
        <button type="button" className="modal-submit" onClick={onConfirm}>
          {t("deleteConfirmConfirm")}
        </button>
      </div>
    </div>
  </div>
);

export default DeleteConfirmModal;
