import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar from "../ProgressBar.jsx";

// Bottom-left banner (below BackgroundJobsBanner / above UpdateBanner):
// surfaces every on-demand runtime pack that isn't installed yet, so
// features that need one are discoverable before you trip over their
// error. The per-feature <RuntimePackGate> is the in-context version of
// the same thing; this is the "you could also get…" list.
//
// Unlike OptionalModelsBanner, this does NOT auto-download — the
// on-demand packs are big (the voice pack ~1 GB) and shouldn't start
// pulling on a metered connection without a click.

const LABELS = {
  llm: "AI text tools (titles, hashtags, transition planning)",
  "python-voice": "Voice & effects (voiceover, cloning, music, diarization)",
};

function mb(bytes) {
  return bytes ? ` · ${Math.round(bytes / 1_000_000)} MB` : "";
}

function RuntimePacksBanner() {
  const [packs, setPacks] = useState([]);
  const [dismissed, setDismissed] = useState(false);
  const [busyId, setBusyId] = useState(null);
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState("");

  async function refresh() {
    try {
      const all = await invoke("list_runtime_components");
      setPacks((Array.isArray(all) ? all : []).filter((c) => c.tier === "on-demand" && !c.installed));
    } catch {
      setPacks([]);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function download(id) {
    setBusyId(id);
    setError("");
    setProgress(null);
    const unlisten = await listen("runtime-component-progress", (event) => {
      if (event.payload?.project_id === id) setProgress(event.payload);
    });
    try {
      await invoke("download_runtime_component", { id });
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      unlisten();
      setBusyId(null);
      setProgress(null);
    }
  }

  if (dismissed || packs.length === 0) return null;

  return (
    <div className="optional-models-banner">
      <p className="optional-models-banner-text">Optional add-ons — download when you need them:</p>
      <ul className="runtime-packs-list">
        {packs.map((c) => (
          <li key={c.id}>
            <span className="runtime-packs-name">
              {LABELS[c.id] || c.id}
              {mb(c.size)}
            </span>
            {busyId === c.id ? (
              <ProgressBar progress={progress} />
            ) : (
              <button type="button" onClick={() => download(c.id)} disabled={!!busyId}>
                Download
              </button>
            )}
          </li>
        ))}
      </ul>
      {error && <pre className="result">{error}</pre>}
      <div className="optional-models-banner-actions">
        <button type="button" className="optional-models-banner-dismiss" onClick={() => setDismissed(true)}>
          {busyId ? "Hide" : "Not now"}
        </button>
      </div>
    </div>
  );
}

export default RuntimePacksBanner;
