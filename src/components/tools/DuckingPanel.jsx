import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import ProgressBar, { formatElapsed } from "../ProgressBar.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];
const AUDIO_FILTERS = [{ name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] }];

// `projectId` is captured wherever a slow (MusicGen, confirmed
// minutes-scale) operation starts, and travels alongside `musicPath` (see
// `musicForProjectId` below) rather than being re-read from the current
// `projectId` prop when `onMusicBedChange` eventually fires -- the prop
// itself can have already moved on to a different project by then, which
// would silently reproduce the exact misattribution bug this whole
// pattern exists to prevent, just one layer removed (via the effect
// below instead of a direct callback).
function DuckingPanel({ videoPath, words, detectedLanguage, projectId, onMusicBedChange }) {
  const [musicPath, setMusicPath] = useState("");
  // Which project `musicPath` actually belongs to -- set together with it
  // everywhere below, never inferred from the current `projectId` prop.
  const [musicForProjectId, setMusicForProjectId] = useState(null);
  // What to show for whatever's currently active -- the raw file path for
  // an upload, the chosen suggestion's own prompt text for a generated
  // one (a temp path there is meaningless to a user, the style it
  // describes is what they actually picked).
  const [activeLabel, setActiveLabel] = useState("");
  const [duckAmount, setDuckAmount] = useState(70); // % reduction during speech
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");

  // A few auditionable suggestions, generated from the transcript, each
  // with its own short preview clip -- picking one is what actually
  // applies it (sets `musicPath`/fires onMusicBedChange below). Replaced
  // the original "generate one bed and use it immediately" flow after
  // direct feedback: committing to whichever single prompt the LLM wrote
  // first, sight (sound) unseen, wasn't good enough.
  const [suggestions, setSuggestions] = useState([]); // [{prompt, previewPath}]
  const [suggesting, setSuggesting] = useState(false);
  const [suggestStatus, setSuggestStatus] = useState("");
  const [suggestError, setSuggestError] = useState("");
  const [finalizingIndex, setFinalizingIndex] = useState(null);
  const [selectedIndex, setSelectedIndex] = useState(null);
  const [finalizeError, setFinalizeError] = useState("");

  // A new video invalidates any music bed/suggestions from the previous
  // one -- same staleness reasoning App.jsx already applies to
  // voiceoverPath/lastBurnedPath/contentIdeas on a video change. Without
  // this, this panel's own local state would keep showing (and the live
  // preview, via onMusicBedChange below, would keep playing) a bed picked
  // for a video that's no longer loaded.
  useEffect(() => {
    setMusicPath("");
    setMusicForProjectId(null);
    setActiveLabel("");
    setSuggestions([]);
    setSelectedIndex(null);
  }, [videoPath]);

  // Lifted to App.jsx so VideoPreview can play this bed live, ducked in
  // real time under the loaded video -- otherwise it's only ever audible
  // after actually running "Add music with ducking" below and opening the
  // resulting export file. Fires on every duck-amount drag too, so the
  // live preview reflects the slider immediately, before committing to a
  // real mix.
  useEffect(() => {
    if (musicForProjectId) onMusicBedChange?.(musicPath, 1 - duckAmount / 100, musicForProjectId);
  }, [musicPath, duckAmount, musicForProjectId, onMusicBedChange]);

  async function pickMusic() {
    const selected = await open({ multiple: false, filters: AUDIO_FILTERS });
    if (typeof selected === "string") {
      setMusicPath(selected);
      setMusicForProjectId(projectId);
      setActiveLabel(selected);
      setSuggestions([]);
      setSelectedIndex(null);
    }
  }

  async function suggestMusic() {
    if (!videoPath || words.length === 0) return;
    setSuggesting(true);
    setSuggestError("");
    setSuggestions([]);
    setSelectedIndex(null);
    setFinalizeError("");
    // Clears whatever bed was active until a new choice is actually made --
    // the old one no longer has a suggestion card backing it, so leaving it
    // "active" with no visible selection would be confusing.
    setMusicPath("");
    setActiveLabel("");
    const unlisten = await listen("music-suggestions-progress", (event) => {
      const { stage, index, total } = event.payload ?? {};
      setSuggestStatus(stage === "deriving_prompts" ? "Coming up with ideas…" : `Generating preview ${index} of ${total}…`);
    });
    try {
      // Lets the model lean on the spoken language's own film/popular
      // music culture (Tamil cinema, English-language albums, etc), not
      // just the transcript's literal content -- see MUSIC_PROMPT_SYSTEM's
      // own doc comment in music_gen.rs for how this was actually verified
      // against the real model, including a real limitation it also found
      // (heavily code-switched Tamil+English content can derail this small
      // model regardless of the label given).
      const result = await invoke("suggest_background_music", { words, language: detectedLanguage || null, projectId });
      setSuggestions(result.map((s) => ({ prompt: s.prompt, previewPath: s.preview_path })));
    } catch (err) {
      setSuggestError(String(err));
    } finally {
      unlisten();
      setSuggesting(false);
      setSuggestStatus("");
    }
  }

  async function chooseSuggestion(index) {
    if (!videoPath || finalizingIndex !== null) return;
    const forProjectId = projectId;
    setFinalizingIndex(index);
    setFinalizeError("");
    try {
      const result = await invoke("finalize_background_music", { videoPath, prompt: suggestions[index].prompt });
      setMusicPath(result.output_path);
      setMusicForProjectId(forProjectId);
      setActiveLabel(result.prompt);
      setSelectedIndex(index);
    } catch (err) {
      setFinalizeError(String(err));
    } finally {
      setFinalizingIndex(null);
    }
  }

  async function run() {
    if (!videoPath || !musicPath || words.length === 0) return;
    const outputPath = await save({ defaultPath: "with-music.mp4", filters: VIDEO_FILTERS });
    if (!outputPath) return;

    setRunning(true);
    setStatus("");
    setProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("ducking-progress", (event) => setProgress(event.payload));
    try {
      const duckLevel = 1 - duckAmount / 100;
      const result = await invoke("duck_music", { videoPath, musicPath, words, duckLevel, outputPath, projectId });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setStatus(`Saved to ${result} in ${elapsed}.`);
    } catch (err) {
      setStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setRunning(false);
      setProgress(null);
    }
  }

  return (
    <div className="inspector-panel">
      <h2>Background music ducking</h2>
      <p className="section-hint">
        Mixes a music bed under the video's existing audio, automatically lowering the music during speech (using the
        transcript timing you already have — no extra audio analysis needed).
      </p>

      <div className="burn-export-buttons-row">
        <button onClick={pickMusic} disabled={suggesting}>
          Choose music file…
        </button>
        <button onClick={suggestMusic} disabled={!videoPath || words.length === 0 || suggesting}>
          {suggesting ? suggestStatus || "Generating suggestions…" : "✨ Suggest music"}
        </button>
      </div>
      {suggesting && (
        <p className="section-hint">CPU music generation is slow — each preview can take roughly a minute.</p>
      )}
      {suggestError && <pre className="result">{suggestError}</pre>}

      {suggestions.length > 0 && (
        <div className="music-suggestions">
          {suggestions.map((s, i) => (
            <div className={selectedIndex === i ? "music-suggestion-card selected" : "music-suggestion-card"} key={i}>
              <p>{s.prompt}</p>
              <audio controls src={convertFileSrc(s.previewPath)} style={{ width: "100%" }} />
              <button onClick={() => chooseSuggestion(i)} disabled={finalizingIndex !== null}>
                {finalizingIndex === i ? "Applying at full length…" : selectedIndex === i ? "✓ In use" : "Use this"}
              </button>
            </div>
          ))}
        </div>
      )}
      {finalizeError && <pre className="result">{finalizeError}</pre>}

      {musicPath && <p className="result">Active bed: {activeLabel}</p>}

      <label className="style-controls">
        Duck amount ({duckAmount}%)
        <input type="range" min={0} max={95} value={duckAmount} onChange={(e) => setDuckAmount(Number(e.target.value))} />
      </label>

      <button onClick={run} disabled={!videoPath || !musicPath || words.length === 0 || running}>
        {running ? "Mixing…" : "Add music with ducking"}
      </button>
      <ProgressBar progress={progress} />
      {status && <pre className="result">{status}</pre>}
    </div>
  );
}

export default DuckingPanel;
