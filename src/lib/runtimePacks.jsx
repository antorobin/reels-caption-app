import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar from "../components/ProgressBar.jsx";

// On-demand runtime packs (`runtime_fetch.rs`, tier "on-demand"): the LLM
// and the voice/effects Python env. The slim installer doesn't ship them;
// they download the first time a feature that needs them is opened. The
// full installer bundles everything, so `list_runtime_components` reports
// them installed and the gate below is a straight passthrough.

const PACK_LABELS = {
  llm: "AI text tools",
  "python-voice": "voice & effects",
};

function mb(bytes) {
  return bytes ? `${Math.round(bytes / 1_000_000)} MB` : "";
}

/// `{ state, progress, error, size, retry }`. `state` is one of
/// "checking" | "ready" | "missing" | "downloading" | "error".
export function useRuntimePack(componentId) {
  const [state, setState] = useState("checking");
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState("");
  const [size, setSize] = useState(0);
  const startedRef = useRef(false);

  const check = useCallback(async () => {
    try {
      const all = await invoke("list_runtime_components");
      const c = Array.isArray(all) ? all.find((x) => x.id === componentId) : null;
      if (!c) {
        // Not in the manifest at all — treat as available (nothing to gate on).
        setState("ready");
        return;
      }
      setSize(c.size || 0);
      setState(c.installed ? "ready" : "missing");
    } catch {
      // Can't check (e.g. running outside Tauri) — don't block the feature.
      setState("ready");
    }
  }, [componentId]);

  useEffect(() => {
    check();
  }, [check]);

  const download = useCallback(async () => {
    if (startedRef.current) return;
    startedRef.current = true;
    setError("");
    setState("downloading");
    setProgress(null);
    const unlisten = await listen("runtime-component-progress", (event) => {
      if (event.payload?.project_id === componentId) setProgress(event.payload);
    });
    try {
      await invoke("download_runtime_component", { id: componentId });
      setState("ready");
    } catch (err) {
      setError(String(err));
      setState("error");
    } finally {
      unlisten();
      startedRef.current = false;
    }
  }, [componentId]);

  return { state, progress, error, size, download, recheck: check };
}

/// Wrap a feature's UI: renders `children` once the pack is present,
/// otherwise a compact download prompt.
export function RuntimePackGate({ need, children }) {
  const { state, progress, error, size, download } = useRuntimePack(need);
  const label = PACK_LABELS[need] || need;

  if (state === "ready") return children;
  if (state === "checking") return <p className="section-hint">Checking for the {label} pack…</p>;

  return (
    <div className="runtime-pack-gate">
      <p className="runtime-pack-gate-text">
        {state === "error"
          ? `Couldn't download the ${label} pack.`
          : state === "downloading"
            ? `Downloading the ${label} pack${size ? ` (${mb(size)})` : ""}…`
            : `This needs the ${label} pack${size ? ` (${mb(size)})` : ""} — a one-time download.`}
      </p>
      {state === "downloading" && <ProgressBar progress={progress} />}
      {error && <pre className="result">{error}</pre>}
      {state !== "downloading" && (
        <button type="button" onClick={download}>
          {state === "error" ? "Retry" : "Download"}
        </button>
      )}
    </div>
  );
}
