type VideoPreviewModalProps = {
  name: string;
  src: string;
  onClose: () => void;
};

// 视频预览弹窗，内置原生播放器。
const VideoPreviewModal = ({ name, src, onClose }: VideoPreviewModalProps) => (
  <div className="modal-backdrop" onClick={onClose}>
    <div
      className="modal image-modal"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="image-modal-header">
        <h3 className="modal-title">{name}</h3>
        <button type="button" className="image-modal-close" onClick={onClose}>
          ×
        </button>
      </div>
      <div className="image-modal-body video-modal-body">
        <video src={src} controls preload="metadata" playsInline />
      </div>
    </div>
  </div>
);

export default VideoPreviewModal;
