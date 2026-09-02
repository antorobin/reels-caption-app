import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar from "../ProgressBar.jsx";

// Tamil transcription and voice cloning need checkpoint files that are
// deliberately not baked into the installer (large, and this app has no
// bundled-conda-env story for the Python packages those features need
// anyway — see model_fetch.rs's module doc comment). This closes just the
// "the model file isn't there" gap: a small dedicated release asset
// (~360MB, distinct from the much bigger developer-facing
// `npm run fetch-resources` archive, which also re-bundles
// ffmpeg/llama/Piper that an installed app already has).
//
// Starts downloading itself the moment it notices something's missing --
// no click required, so a fresh install ends up fully capable without the
// user needing to notice and act on a prompt. Still surfaced (fixed
// bottom-left; Burn & Export owns bottom-right) purely so a slow/metered
// connection isn't a silent mystery, and dismissible without cancelling
// the download itself (there's no cancel endpoint on the Rust side, and a
// half-extracted archive is worse than one left to finish in the
// background). A failure surfaces with a manual Retry rather than
// auto-retrying, since silently hammering the release URL on every
// transient network blip would be its own bad behavior.
function OptionalModelsBanner() {
  const [missing, setMissing] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState("");

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

  useEffect(() => {
    invoke("optional_models_missing")
      .then((isMissing) => {
        setMissing(isMissing);
        if (isMissing) download(); // Auto-start -- see comment above.
      })
      .catch(() => {}); // Best-effort — a check failure just skips the prompt, not a real error.
  }, []);

  if (!missing || dismissed) return null;

  return (
    <div className="optional-models-banner">
      <p className="optional-models-banner-text">
        {error
          ? "Couldn't download Tamil transcription & voice cloning (~360MB) — everything else already works without it."
          : "Downloading Tamil transcription & voice cloning (~360MB) in the background — everything else already works without it."}
      </p>
      {downloading && <ProgressBar progress={progress} />}
      {error && <pre className="result">{error}</pre>}
      <div className="optional-models-banner-actions">
        {error && !downloading && (
          <button type="button" onClick={download}>
            Retry
          </button>
        )}
        <button type="button" className="optional-models-banner-dismiss" onClick={() => setDismissed(true)}>
          {downloading ? "Hide" : "Not now"}
        </button>
      </div>
    </div>
  );
}

export default OptionalModelsBanner;
