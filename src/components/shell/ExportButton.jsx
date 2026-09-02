import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import ScheduleToInstagramButton from "./ScheduleToInstagramButton.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];

// Requested directly: once a video's been saved (burned captions actually
// exist on disk -- `lastBurnedPath`), a single "Export" entry point with
// two options, rather than a standalone "Schedule to Instagram" button
// sitting next to Save. Disabled until there's a real burned file --
// there's nothing to download or publish before that.
//
// "Publish to Instagram" reuses ScheduleToInstagramButton's existing modal
// unchanged (see its own doc comment) -- this menu just controls when it
// opens instead of it owning a trigger button itself.
function ExportButton({ lastBurnedPath, contentIdeas }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [instagramOpen, setInstagramOpen] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState("");

  const disabled = !lastBurnedPath;

  async function handleDownload() {
    setMenuOpen(false);
    setError("");
    const suggestedName = lastBurnedPath.split(/[\\/]/).pop() || "exported-video.mp4";
    const destination = await save({ defaultPath: suggestedName, filters: VIDEO_FILTERS });
    if (!destination) return; // User cancelled the dialog.
    setDownloading(true);
    try {
      await invoke("export_file", { source: lastBurnedPath, destination });
    } catch (err) {
      setError(String(err));
    } finally {
      setDownloading(false);
    }
  }

  function handlePublishToInstagram() {
    setMenuOpen(false);
    setInstagramOpen(true);
  }

  return (
    <div className="export-button-wrap">
      <button
        type="button"
        className="export-button"
        onClick={() => setMenuOpen((v) => !v)}
        disabled={disabled}
        title={disabled ? "Save (burn captions) a video first" : undefined}
      >
        {downloading ? "Exporting…" : "Export"}
      </button>

      {menuOpen && !disabled && (
        <>
          <div className="export-menu-backdrop" onClick={() => setMenuOpen(false)} />
          <div className="export-menu">
            <button type="button" onClick={handleDownload}>
              Download
            </button>
            <button type="button" onClick={handlePublishToInstagram}>
              Publish to Instagram
            </button>
          </div>
        </>
      )}

      {error && <pre className="result">{error}</pre>}

      <ScheduleToInstagramButton
        open={instagramOpen}
        onClose={() => setInstagramOpen(false)}
        videoPath={lastBurnedPath}
        contentIdeas={contentIdeas}
      />
    </div>
  );
}

export default ExportButton;
