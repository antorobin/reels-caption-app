import { convertFileSrc } from "@tauri-apps/api/core";

// Requested directly: the 🎬 icon on any project row (not just the
// currently-open one) opens the real, untouched original video in a
// popup -- an in-app modal, same `.shell-modal-backdrop`/`.shell-modal`
// convention every other popup in this app already uses, rather than a
// real second OS window (that tradeoff was asked and confirmed directly:
// consistency with the rest of the UI over a more "native" but heavier
// separate-window build). Takes a bare `originalPath` -- every row from
// `list_projects` already carries its own, so this has no dependency on
// whichever project happens to be loaded into the live editing session.
function VideoPreviewModal({ title, originalPath, onClose }) {
  return (
    <div className="shell-modal-backdrop" onClick={onClose}>
      <div className="shell-modal video-preview-modal" onClick={(e) => e.stopPropagation()}>
        <div className="shell-modal-header">
          <h2>{title || "Original video"}</h2>
          <button type="button" className="shell-modal-close" onClick={onClose}>
            ✕
          </button>
        </div>
        <video controls autoPlay src={convertFileSrc(originalPath)} className="video-preview-modal-player" />
      </div>
    </div>
  );
}

export default VideoPreviewModal;
