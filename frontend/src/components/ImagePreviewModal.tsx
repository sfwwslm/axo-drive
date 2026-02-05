import { useEffect, useRef } from "react";
import type { FileEntry } from "../types";
import { buildPreviewUrl } from "../utils";

type ImagePreviewModalProps = {
  entries: FileEntry[];
  previewIndex: number;
  onClose: () => void;
  onPrev: () => void;
  onNext: () => void;
};

// 图片预览弹窗，支持滑动与左右切换。
const ImagePreviewModal = ({
  entries,
  previewIndex,
  onClose,
  onPrev,
  onNext,
}: ImagePreviewModalProps) => {
  const previewEntry = entries[previewIndex];
  const touchStartX = useRef<number | null>(null);

  useEffect(() => {
    if (!previewEntry) {
      onClose();
    }
  }, [previewEntry, onClose]);

  if (!previewEntry) return null;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal image-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="image-modal-header">
          <h3 className="modal-title">{previewEntry.name}</h3>
          <button type="button" className="image-modal-close" onClick={onClose}>
            ×
          </button>
        </div>
        <div
          className="image-modal-body"
          onTouchStart={(event) => {
            touchStartX.current = event.touches[0]?.clientX ?? null;
          }}
          onTouchEnd={(event) => {
            const startX = touchStartX.current;
            if (startX === null) return;
            const endX = event.changedTouches[0]?.clientX ?? startX;
            const delta = endX - startX;
            touchStartX.current = null;
            if (Math.abs(delta) < 40) return;
            if (delta > 0) {
              onPrev();
            } else {
              onNext();
            }
          }}
        >
          {entries.length > 1 && (
            <>
              <button
                type="button"
                className="image-nav-btn left"
                onClick={onPrev}
                aria-label="上一张"
              >
                ‹
              </button>
              <button
                type="button"
                className="image-nav-btn right"
                onClick={onNext}
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
  );
};

export default ImagePreviewModal;
