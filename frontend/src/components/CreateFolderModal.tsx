import type { FormEvent } from "react";
import type { MessageKey } from "../i18n";

type CreateFolderModalProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
  folderName: string;
  onFolderNameChange: (value: string) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
};

// 新建目录弹窗。
const CreateFolderModal = ({
  t,
  folderName,
  onFolderNameChange,
  onClose,
  onSubmit,
}: CreateFolderModalProps) => (
  <div className="modal-backdrop" onClick={onClose}>
    <div className="modal" onClick={(event) => event.stopPropagation()}>
      <h3 className="modal-title">{t("createFolder")}</h3>
      <form className="modal-form" onSubmit={onSubmit}>
        <input
          className="modal-input"
          placeholder={t("folderNamePlaceholder")}
          value={folderName}
          onChange={(event) => onFolderNameChange(event.target.value)}
          autoFocus
        />
        <div className="modal-actions">
          <button type="button" className="modal-cancel" onClick={onClose}>
            {t("cancel")}
          </button>
          <button type="submit" className="modal-submit">
            {t("create")}
          </button>
        </div>
      </form>
    </div>
  </div>
);

export default CreateFolderModal;
