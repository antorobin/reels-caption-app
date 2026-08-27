import { useEffect, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

// Checks once on launch for a newer signed release -- see tauri.conf.json's
// `plugins.updater`, which points at a static `latest.json` hosted as a
// GitHub Release asset, the same free hosting this project already uses
// for the MSI/model downloads. Every downloaded update is verified against
// the public key baked in there before it's ever installed; the matching
// private key never ships anywhere (see README's "Over-the-air updates").
// Silent when there's nothing new or the check fails (offline, manifest
// unreachable) -- only appears once an actual, verified update is ready.
function UpdateBanner() {
  const [update, setUpdate] = useState(null);
  const [dismissed, setDismissed] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState("");

  useEffect(() => {
    check().then(setUpdate).catch(() => {});
  }, []);

  async function installAndRestart() {
    if (!update) return;
    setInstalling(true);
    setError("");
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) setProgress(Math.min(100, Math.round((downloaded / total) * 100)));
        }
      });
      await relaunch();
    } catch (err) {
      setError(String(err));
      setInstalling(false);
    }
  }

  if (!update || dismissed) return null;

  return (
    <div className="update-banner">
      <p className="update-banner-text">
        Version {update.version} is available — you're on {update.currentVersion}.
        {update.body && <span className="update-banner-notes"> {update.body}</span>}
      </p>
      {installing && (
        <div className="update-banner-progress">
          <progress value={progress} max="100" />
          <span>{progress}%</span>
        </div>
      )}
      {error && <pre className="result">{error}</pre>}
      <div className="update-banner-actions">
        <button type="button" onClick={installAndRestart} disabled={installing}>
          {installing ? "Installing…" : "Download & Restart"}
        </button>
        <button
          type="button"
          className="update-banner-dismiss"
          onClick={() => setDismissed(true)}
          disabled={installing}
        >
          Later
        </button>
      </div>
    </div>
  );
}

export default UpdateBanner;
