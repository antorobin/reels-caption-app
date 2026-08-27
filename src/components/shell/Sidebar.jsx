import { SUPPORTED_LANGUAGES } from "../../lib/languages.js";

// Media Pool only -- the old long Tools navigation list is gone. Every
// tool now lives contextually in MainPanel (below the video) or behind
// "More options", so there's nothing left to navigate to here.
function Sidebar({ videoPath, onPickVideo }) {
  return (
    <aside className="shell-sidebar">
      <div className="shell-sidebar-heading">Media Pool</div>
      <div className="shell-sidebar-body">
        {videoPath ? (
          <div className="shell-sidebar-thumb">
            <div className="shell-sidebar-thumb-frame">
              <svg width="22" height="22" viewBox="0 0 24 24" fill="var(--shell-text-dim)">
                <path d="M8 5v14l11-7z" />
              </svg>
            </div>
            <div className="shell-sidebar-video-path">{videoPath.split(/[\\/]/).pop()}</div>
          </div>
        ) : (
          <p className="shell-viewer-empty">No video loaded yet.</p>
        )}
        <button onClick={onPickVideo}>{videoPath ? "Choose another video…" : "Choose video…"}</button>
        {videoPath && (
          <p className="shell-sidebar-hint">Everything below runs automatically once a video loads.</p>
        )}
        <p className="shell-sidebar-hint">
          Supported languages: {SUPPORTED_LANGUAGES.join(", ")} — detected automatically per speech segment, even
          within one video (e.g. Tamil + English), no need to pick one.
        </p>
      </div>
    </aside>
  );
}

export default Sidebar;
