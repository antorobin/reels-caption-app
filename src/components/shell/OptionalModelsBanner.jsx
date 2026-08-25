import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar from "../ProgressBar.jsx";

// Tamil transcription and voice cloning need checkpoint files that are
// deliberately not baked into the installer (large, and this app has no
// bundled-conda-env story for the Python packages those features need
// anyway — see model_fetch.rs's module doc comment). This closes just the
// "the model file isn't there" gap: a one-click download of a small
// dedicated release asset (~360MB, distinct from the much bigger
// developer-facing `npm run fetch-resources` archive, which also
// re-bundles ffmpeg/llama/Piper that an installed app already has).
//
// Fixed bottom-left (Burn & Export owns bottom-right) so it never blocks
// the main workflow — dismissible, and only shown at all if
// `optional_models_missing` says something's actually missing.
function OptionalModelsBanner() {
  const [missing, setMissing] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke("optional_models_missing")
      .then(setMissing)
      .catch(() => {}); // Best-effort — a check failure just skips the prompt, not a real error.
  }, []);

  async function download() {
    setDownloading(true);
    setError("");
    setProgress(null);
    const unlisten = await listen("optional-models-progress", (event) => setProgress(event.payload));
    try {
      await invoke("download_optional_models");
      setMissing(false);
    } catch (err) {
      setError(String(err));
    } finally {
      unlisten();
      setDownloading(false);
      setProgress(null);
    }
  }

  if (!missing || dismissed) return null;

  return (
    <div className="optional-models-banner">
      <p className="optional-models-banner-text">
        Tamil transcription and voice cloning need one more download (~360MB) to work — everything else already
        works without it.
      </p>
      {downloading && <ProgressBar progress={progress} />}
      {error && <pre className="result">{error}</pre>}
      <div className="optional-models-banner-actions">
        <button type="button" onClick={download} disabled={downloading}>
          {downloading ? "Downloading…" : "Download now"}
        </button>
        <button type="button" className="optional-models-banner-dismiss" onClick={() => setDismissed(true)} disabled={downloading}>
          Not now
        </button>
      </div>
    </div>
  );
}

export default OptionalModelsBanner;
