import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import AuthScreen from "./components/auth/AuthScreen.jsx";
import RuntimeSetup from "./components/RuntimeSetup.jsx";
import { defaultCaptionStyle } from "./components/CaptionStyleEditor.jsx";
import { defaultTheme } from "./lib/themes.js";
import { formatElapsed } from "./components/ProgressBar.jsx";
import AppShell from "./components/shell/AppShell.jsx";
import { useAuth } from "./context/AuthContext.jsx";
import { placeholderTitleFor } from "./lib/library.js";
import {
  cancelProjectTimers,
  configureAutosave,
  emptyProjectSlice,
  flushProjectSave,
  getState,
  removeProjectSlice,
  setCurrentProjectId,
  setProjectSlice,
  upsertProjectJob,
  upsertProjectSlice,
  useCurrentProjectId,
  useProjectSlice,
} from "./state/projectStore.js";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];

// Shown while no project is open at all -- computed once at module load
// (not per-render) so its object identity stays stable across renders;
// nothing downstream should ever mistake this for a real, savable project
// (its `id` is "").
const EMPTY_PROJECT_SLICE = emptyProjectSlice("", defaultCaptionStyle());

const FACTORY_CAPTION_THEME_ID = defaultTheme().id;

// The signed-in user's saved default caption style (library.rs's
// `get_default_caption_style`), fetched once per sign-in (see the
// `useEffect` below keyed on `user?.uid`) and cached here at module scope --
// mirrors `configureAutosave`'s own module-level-cache pattern just above,
// for the same reason: `projectSliceFromProject` is a plain function, not a
// hook, so it has no other way to see this without threading it through
// every call site. `null` means "not fetched yet, or the user has never
// customized a default" -- either way, fall back to the hardcoded factory
// default exactly as if this feature didn't exist.
let cachedUserDefaultCaptionStyle = null;

// Turns a `Project` row (library.rs's shape) into this store's per-project
// slice shape -- the direct replacement for the old `applyProjectState`'s
// field-by-field `setXxx` calls. `project.state` is the opaque JSON blob
// (see library.rs's own doc comment); everything else comes from the
// row's real columns.
function projectSliceFromProject(project) {
  const state = project.state || {};
  return {
    id: project.id,
    createdAt: project.created_at,
    originalFilename: project.original_filename,
    title: project.title,
    description: project.description,
    hashtags: project.hashtags || [],
    videoPath: project.original_path,
    originalVideoPath: project.original_path,
    lastBurnedPath: project.last_burned_path || "",
    words: state.words || [],
    detectedLanguage: state.detectedLanguage ?? null,
    captionStyle: state.captionStyle || cachedUserDefaultCaptionStyle?.style || defaultCaptionStyle(),
    captionThemeId: state.captionThemeId || cachedUserDefaultCaptionStyle?.theme_id || FACTORY_CAPTION_THEME_ID,
    // Per-portion style overrides are per-project only -- deliberately
    // not part of the saved user default (a default is "the style I
    // start a new video with," which has no meaning for "a style that
    // only applies to seconds 12-18 of THIS video").
    captionStyleOverrides: state.captionStyleOverrides || [],
    // Real burned-into-the-footage effects (zoom punch / flash cut) at a
    // point in time -- also per-project only, for the same reason as
    // captionStyleOverrides just above.
    videoTransitions: state.videoTransitions || [],
    prosody: state.prosody || [],
    speakers: state.speakers || [],
    voiceoverPath: state.voiceoverPath || "",
    voiceoverOffset: state.voiceoverOffset || 0,
    musicPath: state.musicPath || "",
    duckLevel: state.duckLevel ?? 0.3,
    contentIdeas: state.contentIdeas || null,
    contentStrategyOptions: state.contentStrategyOptions || [],
    contentHints: state.contentHints || "",
    // Transient job state always starts fresh on (re)load -- there's
    // nothing "still running" about a project that was just hydrated
    // from disk, regardless of what it looked like last time it was open.
    jobs: emptyProjectSlice(project.id).jobs,
  };
}

// The single source of truth for "what a saved project row looks like
// right now," given one project's store slice -- shared by autosave
// (projectStore.js calls this automatically, see `configureAutosave`
// below) and the explicit, immediate save that follows a successful burn,
// so the two never drift into building slightly different payloads.
function projectPayloadFrom(slice, overrides = {}) {
  return {
    id: slice.id,
    original_path: slice.videoPath,
    original_filename: slice.originalFilename,
    title: slice.title,
    description: slice.description,
    hashtags: slice.hashtags,
    last_burned_path: slice.lastBurnedPath || null,
    storage_location: "local",
    created_at: slice.createdAt,
    updated_at: 0, // Rust re-stamps this on every save; the value here is never read back.
    state: {
      words: slice.words,
      detectedLanguage: slice.detectedLanguage,
      captionStyle: slice.captionStyle,
      captionThemeId: slice.captionThemeId,
      captionStyleOverrides: slice.captionStyleOverrides,
      videoTransitions: slice.videoTransitions,
      prosody: slice.prosody,
      speakers: slice.speakers,
      voiceoverPath: slice.voiceoverPath,
      voiceoverOffset: slice.voiceoverOffset,
      musicPath: slice.musicPath,
      duckLevel: slice.duckLevel,
      contentIdeas: slice.contentIdeas,
      contentStrategyOptions: slice.contentStrategyOptions,
      contentHints: slice.contentHints,
    },
    ...overrides,
  };
}

// Registered once for the app's whole lifetime (not per-render, not per-
// component-instance) -- every `upsertProjectSlice`/`setProjectSlice` call
// anywhere triggers autosave automatically through this, so no
// call site (background job or synchronous UI edit alike) has to
// remember to schedule a save itself. See projectStore.js's own doc
// comment on `configureAutosave` for why this is safer than the
// alternative.
configureAutosave(invoke, projectPayloadFrom);

function App() {
  const videoRef = useRef(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const [importing, setImporting] = useState(false);
  const [importProgress, setImportProgress] = useState(null);

  // The one piece of "which project" state that's still real React state
  // (via the store's own selector hook) -- everything else about a
  // project now lives in its own slice in projectStore.js, addressable by
  // id regardless of whether it's the one currently displayed. See
  // projectStore.js's own module doc comment for why this replaces the
  // `projectIdRef`-guard pattern rather than just extending it further.
  const currentProjectId = useCurrentProjectId();
  const slice = useProjectSlice(currentProjectId) ?? EMPTY_PROJECT_SLICE;

  const {
    videoPath,
    originalVideoPath,
    lastBurnedPath,
    words,
    detectedLanguage,
    captionStyle,
    captionThemeId,
    captionStyleOverrides,
    videoTransitions,
    prosody,
    speakers,
    voiceoverPath,
    voiceoverOffset,
    musicPath,
    duckLevel,
    contentIdeas,
    contentStrategyOptions,
    contentHints,
    jobs,
  } = slice;
  const projectCreatedAt = slice.createdAt;
  const projectOriginalFilename = slice.originalFilename;
  const projectTitle = slice.title;
  const projectDescription = slice.description;
  const projectHashtags = slice.hashtags;

  const pipelineRunning = jobs.pipeline.running;
  const pipelineStatus = jobs.pipeline.status;
  const pipelineProgress = jobs.pipeline.progress;
  const analyzingProsody = jobs.prosody.running;
  const prosodyStatus = jobs.prosody.status;
  const prosodyProgress = jobs.prosody.progress;
  const analyzingSpeakers = jobs.diarize.running;
  const diarizeStatus = jobs.diarize.status;
  const diarizeProgress = jobs.diarize.progress;
  const burning = jobs.burn.running;
  const burnStatus = jobs.burn.status;
  const burnProgress = jobs.burn.progress;
  const generatingContentIdeas = jobs.contentIdeas.running;
  const contentIdeasError = jobs.contentIdeas.error;
  const suggestingTransitionPlan = jobs.transitionPlan.running;
  const transitionPlanError = jobs.transitionPlan.error;

  // Convenience setters bound to *whichever project is currently
  // displayed* -- safe only for synchronous, user-driven edits made while
  // looking at that project (typing a title, editing a transcript word,
  // dragging a style slider). Every async/background operation below
  // writes through `upsertProjectSlice(forProjectId, ...)` directly
  // instead, addressed by the id it was actually started for -- never
  // through these, which would silently re-introduce the exact
  // misattribution bug this store exists to prevent if used from a
  // completion handler that can outlive a project switch.
  function patchCurrentProject(patch) {
    if (!currentProjectId) return;
    upsertProjectSlice(currentProjectId, patch);
  }
  const setWords = (updater) =>
    patchCurrentProject((prev) => ({ words: typeof updater === "function" ? updater(prev.words) : updater }));
  const setCaptionStyle = (updater) =>
    patchCurrentProject((prev) => ({ captionStyle: typeof updater === "function" ? updater(prev.captionStyle) : updater }));
  const setCaptionThemeId = (id) => patchCurrentProject({ captionThemeId: id });
  // Non-overlap is already enforced before this is ever called
  // (CaptionOverridePanel.jsx disables Apply via overrideRangeOverlaps) --
  // this just appends.
  const addCaptionStyleOverride = (override) =>
    patchCurrentProject((prev) => ({ captionStyleOverrides: [...(prev.captionStyleOverrides || []), override] }));
  const removeCaptionStyleOverride = (index) =>
    patchCurrentProject((prev) => ({ captionStyleOverrides: (prev.captionStyleOverrides || []).filter((_, i) => i !== index) }));
  // Replaces one existing override in place (its range and/or style) --
  // "Change style" on an already-applied override, as opposed to
  // addCaptionStyleOverride's "add a new one" -- same non-overlap
  // guarantee already holds since the edited range's own overlap check
  // (CaptionOverridePanel.jsx, excluding this override's own index) ran
  // before this is ever called.
  const updateCaptionStyleOverride = (index, override) =>
    patchCurrentProject((prev) => ({
      captionStyleOverrides: (prev.captionStyleOverrides || []).map((o, i) => (i === index ? override : o)),
    }));
  // A real burned-into-the-footage effect at a point in time -- see
  // video_transitions.rs. `{ time, effect }` where effect is one of
  // "zoom-punch" | "flash-cut" | "shake" | "color-pulse", matching
  // TransitionEffect's #[serde(rename_all = "kebab-case")] on the Rust
  // side exactly.
  const addVideoTransition = (transition) =>
    patchCurrentProject((prev) => ({ videoTransitions: [...(prev.videoTransitions || []), transition] }));
  const removeVideoTransition = (index) =>
    patchCurrentProject((prev) => ({ videoTransitions: (prev.videoTransitions || []).filter((_, i) => i !== index) }));
  // Changes an existing transition's own effect in place (its "Edit
  // transition" action) -- same non-add/non-remove shape as
  // updateCaptionStyleOverride above.
  const updateVideoTransition = (index, transition) =>
    patchCurrentProject((prev) => ({
      videoTransitions: (prev.videoTransitions || []).map((t, i) => (i === index ? transition : t)),
    }));
  const setContentHints = (v) => patchCurrentProject({ contentHints: v });
  const setProjectTitle = (v) => patchCurrentProject({ title: v });
  const setProjectDescription = (v) => patchCurrentProject({ description: v });
  const setProjectHashtags = (v) => patchCurrentProject({ hashtags: v });

  const { user, loading: authLoading } = useAuth();

  // Slim-installer first-run gate: `runtime_fetch.rs`'s `tier: "core"`
  // components (minimal ffmpeg + the stt Python env) must be present
  // before the editor is any use. `null` = still checking, `[]` = ready,
  // a non-empty array = show <RuntimeSetup>. The full installer bundles
  // these, so the check comes back `[]` there and nothing renders.
  const [missingCore, setMissingCore] = useState(null);
  useEffect(() => {
    if (!user) return;
    invoke("missing_core_components")
      .then((missing) => setMissingCore(Array.isArray(missing) ? missing : []))
      .catch(() => setMissingCore([])); // check failed -> don't trap the user on the setup screen
  }, [user]);

  // Fetches this signed-in user's saved default caption style once per
  // sign-in, caching it at module scope (`cachedUserDefaultCaptionStyle`)
  // for `projectSliceFromProject` to read synchronously when the *next*
  // new project is created -- a project already open when this resolves is
  // deliberately left alone (its own already-loaded style is never
  // retroactively swapped out from under the user just because a fetch
  // landed). A brand-new project created in the brief window before this
  // resolves falls back to the hardcoded factory default, same as if the
  // user had never customized one -- an acceptable, rare edge case rather
  // than blocking project creation on this fetch.
  useEffect(() => {
    if (!user?.uid) {
      cachedUserDefaultCaptionStyle = null;
      return;
    }
    let cancelled = false;
    invoke("get_default_caption_style", { userId: user.uid })
      .then((saved) => {
        if (!cancelled) cachedUserDefaultCaptionStyle = saved;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [user?.uid]);

  // Debounced (mirrors AUTOSAVE_DEBOUNCE_MS's own 1s convention in
  // projectStore.js): saves the *currently open* project's caption style
  // as this user's new default every time it actually changes, whether
  // from picking a different theme card or hand-tweaking a field --
  // "once we made a change on a theme, persist it and use it as default
  // next time," per the user's own request. Guarded on a real project
  // being open (skip the empty placeholder slice) and a signed-in user
  // (there's no per-user row to save against otherwise).
  useEffect(() => {
    if (!user?.uid || !currentProjectId) return undefined;
    const timer = setTimeout(() => {
      invoke("save_default_caption_style", { userId: user.uid, themeId: captionThemeId, style: captionStyle }).catch(() => {});
    }, 1000);
    return () => clearTimeout(timer);
  }, [user?.uid, currentProjectId, captionStyle, captionThemeId]);

  // The "Reset to factory default" escape hatch: reverts the *currently
  // open* project's style back to Cascade Bold immediately (so the editor
  // reflects it right away), and clears the saved default server-side so
  // the *next* new project also starts from the factory default again,
  // instead of whatever was customized before. Does not touch any other
  // project's own already-persisted style -- only this project's live
  // state and the user-level default going forward.
  function resetCaptionStyleToFactoryDefault() {
    patchCurrentProject({ captionStyle: defaultCaptionStyle(), captionThemeId: FACTORY_CAPTION_THEME_ID });
    if (user?.uid) invoke("reset_default_caption_style", { userId: user.uid }).catch(() => {});
  }

  // Splashscreen shows natively the instant the process starts (see
  // tauri.conf.json) so there's no blank window while the webview spins
  // up. A short minimum delay here keeps it from flashing away instantly
  // on fast machines — this app mounts in well under that.
  // Result of the global-hotkey dictation HUD (dictation.rs + DictationHud.jsx)
  // -- that window has no idea whether a video is loaded here, so this is
  // where "no video loaded" actually gets surfaced instead of silently
  // discarding the recording. Re-subscribed whenever `videoPath` changes
  // so the closure's check is never against a stale value.
  useEffect(() => {
    const unlistenPromise = listen("dictation-result", (event) => {
      if (!videoPath) {
        setPipelineStatusFallback("Dictation captured, but no video is loaded to attach captions to — open one first.");
        return;
      }
      handleCaptionsFromRecording(event.payload?.words);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- videoPath/currentProjectId read fresh via closure re-subscription
  }, [videoPath, currentProjectId]);

  // A tiny helper for the one status message above that can legitimately
  // fire with *no* project open at all (dictating with nothing loaded) --
  // `patchCurrentProject` is a no-op in that case, so this just surfaces
  // the message via a local fallback state instead of trying to store it
  // on a project that doesn't exist. Cleared the moment any project
  // becomes current -- it only ever means something in the "nothing
  // open" state.
  const [noProjectStatus, setPipelineStatusFallback] = useState("");
  useEffect(() => {
    if (currentProjectId) setPipelineStatusFallback("");
  }, [currentProjectId]);

  useEffect(() => {
    const timer = setTimeout(() => {
      invoke("close_splashscreen").catch(() => {});
    }, 400);
    return () => clearTimeout(timer);
  }, []);

  // Seeds the editable title/description/hashtags from a freshly generated
  // AI suggestion -- but only while the title still reads as the
  // auto-generated placeholder (see lib/library.js), so this never
  // clobbers a title the user actually typed themselves. Reads/writes the
  // *currently displayed* project only -- if this fires for a project
  // that's no longer current, the next render after switching back to it
  // re-evaluates against its own (by-then-current) contentIdeas/title.
  useEffect(() => {
    if (contentIdeas && projectTitle === placeholderTitleFor(projectOriginalFilename)) {
      patchCurrentProject({
        title: contentIdeas.title,
        description: contentIdeas.description,
        hashtags: contentIdeas.hashtags || [],
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only re-run when contentIdeas itself changes
  }, [contentIdeas]);

  // Multi-select: picking several files at once imports and starts
  // processing all of them, not just the first -- each `create_project`
  // call is sequential (a real file copy per video, and only one copy is
  // ever in flight, so `import-progress` events need no per-project
  // filtering here the way pipeline/burn/etc. events do), but the
  // *pipeline* each one kicks off is fired without `await`, so all of
  // them run concurrently once their own copy finishes, naturally
  // throttled by the backend's heavy-ML semaphore (concurrency.rs) rather
  // than one video's transcription blocking the next video's import.
  // The first file picked becomes the displayed project; the rest process
  // in the background (see Sidebar.jsx's per-row badges and the
  // "N processing" strip for how their progress stays visible).
  async function pickVideo() {
    const selected = await open({ multiple: true, filters: VIDEO_FILTERS });
    const paths = Array.isArray(selected) ? selected : typeof selected === "string" ? [selected] : [];
    if (paths.length === 0) return;

    setImporting(true);
    setImportProgress(null);
    const unlisten = await listen("import-progress", (event) => setImportProgress(event.payload));
    try {
      let isFirst = true;
      for (const path of paths) {
        try {
          // Copies the file into app-managed local storage and creates its
          // library row -- see library.rs's own doc comment for why this
          // (never the OS path the dialog returned) is what every downstream
          // command operates on from here on.
          const project = await invoke("create_project", { videoPath: path });
          setProjectSlice(project.id, projectSliceFromProject(project));
          if (isFirst) {
            setCurrentProjectId(project.id);
            setCurrentTime(0);
            setDuration(0);
            isFirst = false;
          }
          // Auto-starts the moment each video's own copy finishes -- no
          // separate "Run pipeline" step, same as before.
          runPipelineFor(project.original_path, project.id);
        } catch (err) {
          // One bad file (corrupt, permissions, ...) in a multi-select
          // shouldn't abort the rest of the batch.
          setPipelineStatusFallback(`Error importing ${path.split(/[\\/]/).pop()}: ${err}`);
        }
      }
    } finally {
      unlisten();
      setImporting(false);
      setImportProgress(null);
    }
  }

  // Wired to the sidebar's project list -- unlike pickVideo, this never
  // re-runs the transcription pipeline: words/captionStyle/etc. are
  // already saved in project.state from a previous session.
  //
  // Deliberately re-fetches the project fresh via `get_project` instead of
  // trusting the `project` object the sidebar passed in -- confirmed
  // directly as a real bug, not a hypothetical: Sidebar.jsx's own project
  // list is only refreshed on mount and right after a new import, never
  // after an autosave tick, a burn, or a content-strategy generation, so
  // by the time a project is clicked (including the one already open) its
  // cached row can be many minutes stale. Applying that stale snapshot
  // over a live editing session silently discarded already-generated
  // transcript/content-ideas/burned-video state, and the very next
  // autosave then persisted that reversion as if it were real. Re-clicking
  // the project that's already open is a no-op (nothing to load that
  // isn't already live in memory, and reloading it would only risk
  // clobbering unsaved-but-in-flight progress with whatever's on disk).
  //
  // If this project already has a resident slice in the store (a
  // background job for it is running, or it was opened earlier this
  // session), that in-memory copy is provably at least as current as
  // whatever `get_project` would return (every write here goes through
  // the store first, then autosaves to disk a moment later) -- so this
  // only re-fetches for a project with no resident slice yet.
  async function loadProject(project) {
    if (project.id === currentProjectId) return;
    setCurrentTime(0);
    setDuration(0);
    if (getState().projects[project.id]) {
      setCurrentProjectId(project.id);
      return;
    }
    try {
      const fresh = await invoke("get_project", { id: project.id });
      setProjectSlice(fresh.id, projectSliceFromProject(fresh));
    } catch (err) {
      // Fresh fetch failed (e.g. a transient DB hiccup) -- fall back to the
      // sidebar's copy rather than leaving the click with no effect at all.
      setProjectSlice(project.id, projectSliceFromProject(project));
    }
    setCurrentProjectId(project.id);
  }

  // Deletes the *currently open* project -- requested directly to live in
  // the project view rather than as a per-row action in the sidebar list,
  // with its own confirmation. Removes its slice from the store entirely
  // (rather than resetting fields back to blank) and cancels any pending
  // autosave timer for it, so a stale timer can't try to save a
  // project that no longer exists a moment later.
  async function deleteCurrentProject() {
    if (!currentProjectId) return;
    if (!confirm("Delete this project and its stored video files? This can't be undone.")) return;
    const idToDelete = currentProjectId;
    try {
      await invoke("delete_project", { id: idToDelete });
      cancelProjectTimers(idToDelete);
      removeProjectSlice(idToDelete);
      setCurrentProjectId("");
      setCurrentTime(0);
      setDuration(0);
    } catch (err) {
      setPipelineStatusFallback(`Error deleting project: ${err}`);
    }
  }

  // `forProjectId` is captured by VoiceoverSection.jsx at the start of its
  // (slow: TTS + optional voice cloning + re-transcription) operation.
  // Writes into `forProjectId`'s own slice directly -- correct regardless
  // of whether that project is the one currently displayed, since there's
  // no shared mutable location for two projects' results to collide on
  // anymore (see projectStore.js's own doc comment).
  function handleVoiceoverReady(path, offsetSeconds, voiceoverWords, forProjectId) {
    const offset = offsetSeconds ?? 0;
    upsertProjectSlice(forProjectId, (prev) => {
      const patch = { voiceoverPath: path, voiceoverOffset: offset };
      // The voiceover replaces the video's own audio, so its transcript
      // (real word-level timestamps from re-transcribing it -- see
      // VoiceoverSection.jsx) becomes the working transcript too: what
      // Burn & Export burns, and what the Transcript panel/Timeline show.
      // Shifted by `offset` so timestamps land where the voiceover audio
      // actually plays against the video, same as the live-preview sync.
      // If re-transcription failed, the *audio* swap still applies in
      // Burn & Export (passed via voiceoverPath below) -- just leave the
      // existing transcript in place rather than losing captions entirely.
      if (voiceoverWords) {
        patch.words = voiceoverWords.map((w) => ({ ...w, start: w.start + offset, end: w.end + offset }));
        // Prosody/diarization were computed against the old transcript's
        // word count and timing -- stale now, would silently mismatch if
        // burned alongside the new captions.
        patch.prosody = [];
        patch.speakers = [];
      }
      return patch;
    });
    if (voiceoverWords) {
      upsertProjectJob(forProjectId, "prosody", { status: "" });
      upsertProjectJob(forProjectId, "diarize", { status: "" });
    }
  }

  // Same shape as `handleVoiceoverReady` above -- `forProjectId` is
  // captured by DuckingPanel.jsx at the start of its (slow: MusicGen) call.
  function handleMusicBedChange(path, level, forProjectId) {
    upsertProjectSlice(forProjectId, { musicPath: path, duckLevel: level });
  }

  function handleSeek(t) {
    if (videoRef.current) {
      videoRef.current.currentTime = t;
    }
    setCurrentTime(t);
  }

  function handleWordChange(index, text) {
    setWords((prev) => prev.map((w, i) => (i === index ? { ...w, word: text } : w)));
  }

  function handleWordDelete(index) {
    setWords((prev) => prev.filter((_, i) => i !== index));
  }

  // `forProjectId` is captured by the caller (`pickVideo`) at the moment
  // this project became current, rather than read from `currentProjectId`
  // state here -- this whole function is async and can easily still be
  // running after the user has switched to a different project (or
  // imported another one) in the meantime. Every result below writes into
  // `forProjectId`'s own slice directly, correct regardless of what's
  // currently displayed.
  async function runPipelineFor(path, forProjectId) {
    upsertProjectJob(forProjectId, "pipeline", { running: true, status: "", progress: null });
    upsertProjectSlice(forProjectId, { detectedLanguage: null });
    const startedAt = Date.now();
    // Covers both a single-language video and one that switches languages
    // mid-recording (e.g. Tamil + English) -- detected automatically on
    // the backend (mixed_language.rs), no separate mode to pick here.
    const unlisten = await listen("pipeline-progress", (event) => {
      if (event.payload?.project_id === forProjectId) upsertProjectJob(forProjectId, "pipeline", { progress: event.payload });
    });
    try {
      // Tanglish slang normalization is available in the backend
      // (`slang::normalize_words`) but not exposed in this UI right now --
      // deliberately, to keep the default flow to as few decisions as
      // possible. Wire a toggle back in (e.g. in MoreOptionsModal) if it
      // turns out to be missed.
      const result = await invoke("run_pipeline", { videoPath: path, normalizeSlang: false, projectId: forProjectId });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      // A silent video (see pipeline.rs's has_audio_stream check) comes
      // back with an empty transcript, not an error -- worth a distinct
      // message rather than the slightly odd "Transcribed 0 words."
      upsertProjectSlice(forProjectId, { words: result.words, detectedLanguage: result.detected_language ?? null });
      upsertProjectJob(forProjectId, "pipeline", {
        status:
          result.words.length > 0
            ? `Transcribed ${result.words.length} words in ${elapsed}.`
            : "No audio track found on this video — add a voice-over below to add captions.",
      });
      // Saved immediately, not just via the store's own debounced
      // autosave -- closing the app within that debounce's ~1s window
      // right after transcription finishes would otherwise lose the
      // transcript entirely (a pending `setTimeout` never fires once the
      // whole process is gone). Same reasoning `burnCaptions` already
      // applied to a burned video's own completion.
      flushProjectSave(forProjectId);
      // Auto-fills the project's title/description/hashtags the moment
      // there's a real transcript to work from -- no manual "Generate"
      // click needed for a brand-new import with audio. Fired without
      // `await` so it genuinely runs in the background alongside the
      // pipeline wrapping up, and the seeding effect further down only
      // ever applies its result while the title still reads as the
      // auto-generated placeholder, so it can never clobber a title
      // someone's already edited. A silent video (empty transcript) has
      // nothing to generate from, so this deliberately does nothing then.
      if (result.words.length > 0) {
        generateContentIdeas(result.words, forProjectId);
      }
    } catch (err) {
      upsertProjectJob(forProjectId, "pipeline", { status: `Error: ${err}` });
    } finally {
      unlisten();
      upsertProjectJob(forProjectId, "pipeline", { running: false, progress: null });
    }
  }

  // The manual "Transcribe" button in TranscriptPanel.jsx -- a real gap
  // this closes: there's no cross-restart resumability for a job that was
  // still running when the app closed (a deliberate scope cut, see
  // projectStore.js's own doc comment), so a video whose transcription
  // never finished had no way back to a transcript short of re-importing
  // the whole video as a brand-new project. Re-invokes the exact same
  // `runPipelineFor` a fresh import already uses, against the video
  // that's already on disk for this project -- nothing new on the
  // backend, purely a manual retry entry point for an existing capability.
  function retranscribe() {
    if (!videoPath || !currentProjectId || pipelineRunning) return;
    runPipelineFor(videoPath, currentProjectId);
  }

  async function analyzeProsody() {
    if (!videoPath || words.length === 0) return;
    const forProjectId = currentProjectId;
    upsertProjectJob(forProjectId, "prosody", { running: true, status: "", progress: null });
    const startedAt = Date.now();
    const unlisten = await listen("prosody-progress", (event) => {
      if (event.payload?.project_id === forProjectId) upsertProjectJob(forProjectId, "prosody", { progress: event.payload });
    });
    try {
      const result = await invoke("analyze_prosody", { videoPath, words, projectId: forProjectId });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      upsertProjectSlice(forProjectId, { prosody: result });
      upsertProjectJob(forProjectId, "prosody", { status: `Analyzed ${result.length} words in ${elapsed}.` });
      flushProjectSave(forProjectId); // see runPipelineFor's own comment on why this can't just wait for the debounce
    } catch (err) {
      upsertProjectJob(forProjectId, "prosody", { status: `Error: ${err}` });
    } finally {
      unlisten();
      upsertProjectJob(forProjectId, "prosody", { running: false, progress: null });
    }
  }

  async function diarizeSpeakers() {
    if (!videoPath || words.length === 0) return;
    const forProjectId = currentProjectId;
    upsertProjectJob(forProjectId, "diarize", { running: true, status: "", progress: null });
    const startedAt = Date.now();
    const unlisten = await listen("diarize-progress", (event) => {
      if (event.payload?.project_id === forProjectId) upsertProjectJob(forProjectId, "diarize", { progress: event.payload });
    });
    try {
      const result = await invoke("diarize_speakers", { videoPath, words, projectId: forProjectId });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      upsertProjectSlice(forProjectId, { speakers: result });
      const speakerCount = new Set(result.map((s) => s.speaker_id)).size;
      upsertProjectJob(forProjectId, "diarize", {
        status: `Found ${speakerCount} speaker${speakerCount === 1 ? "" : "s"} across ${result.length} segments in ${elapsed}.`,
      });
      flushProjectSave(forProjectId); // see runPipelineFor's own comment on why this can't just wait for the debounce
    } catch (err) {
      upsertProjectJob(forProjectId, "diarize", { status: `Error: ${err}` });
    } finally {
      unlisten();
      upsertProjectJob(forProjectId, "diarize", { running: false, progress: null });
    }
  }

  // `wordsOverride` lets `runPipelineFor` above trigger this right after a
  // fresh transcription completes, passing the just-received result
  // directly. The manual "Generate" button in ContentIdeasPanel calls this
  // with no argument, falling back to state (the currently displayed
  // project's own words) as it always has.
  //
  // Returns 3 distinct strategy angles (contentStrategyOptions), not one
  // fixed result -- the first is auto-selected into `contentIdeas` so the
  // existing "already filled in by the time you look" auto-fill behavior
  // (see the seeding effect above) keeps working with no further change;
  // picking a different option in ContentIdeasPanel just calls
  // `selectContentStrategyOption` below to swap which one is active.
  // `contentHints` (free text) rides along every call -- empty is fine
  // when there's a transcript to work from, but is the *only* seed when
  // `wordsToUse` is empty (the from-scratch, no-video-upload case).
  //
  // `forProjectId` defaults to whatever's current at call time (the manual
  // "Generate" button in ContentIdeasPanel), or is passed explicitly by
  // `runPipelineFor` (the project it was just transcribing for, which may
  // no longer be the open one by the time this resolves). Reads
  // `forProjectId`'s own `words`/`contentHints`/`detectedLanguage`
  // straight from the store instead of the top-level destructured
  // (current-display-only) variables -- reading those here would silently
  // use *whatever project is currently displayed*'s hints/language for a
  // background project's generation, the exact misattribution class this
  // whole store exists to eliminate, just relocated into this one
  // function instead of fixed by it. `wordsOverride` is only needed
  // because `runPipelineFor` calls this in the same tick its own
  // `upsertProjectSlice` write lands, before this render's closures would
  // otherwise see it.
  async function generateContentIdeas(wordsOverride, forProjectId = currentProjectId) {
    if (!forProjectId) return;
    const targetSlice = getState().projects[forProjectId];
    const wordsToUse = wordsOverride ?? targetSlice?.words ?? [];
    const hints = targetSlice?.contentHints ?? "";
    if (wordsToUse.length === 0 && !hints.trim()) return;
    upsertProjectJob(forProjectId, "contentIdeas", { running: true, error: "" });
    try {
      const result = await invoke("suggest_content_strategy", {
        words: wordsToUse,
        hints: hints.trim() || null,
        language: targetSlice?.detectedLanguage || null,
      });
      upsertProjectSlice(forProjectId, { contentStrategyOptions: result, contentIdeas: result[0] ?? null });
      flushProjectSave(forProjectId); // see runPipelineFor's own comment on why this can't just wait for the debounce
    } catch (err) {
      upsertProjectJob(forProjectId, "contentIdeas", { error: String(err) });
    } finally {
      upsertProjectJob(forProjectId, "contentIdeas", { running: false });
    }
  }

  function selectContentStrategyOption(option) {
    patchCurrentProject({ contentIdeas: option });
  }

  // LLM-refined transition planning (transition_planner.rs) -- given the
  // heuristic's own candidate points (Timeline.jsx's suggestTransitionPoints,
  // times only; the backend re-derives everything else including which
  // ones actually deserve a transition from the real transcript text),
  // adds a real VideoTransition for whichever ones the model kept. Writes
  // through `upsertProjectSlice(forProjectId, ...)` directly rather than
  // `addVideoTransition` (bound to `patchCurrentProject`, i.e. whichever
  // project is *currently displayed*) -- this is an async, several-second
  // LLM call, so the same "never assume the project you started this for
  // is still the one on screen when it resolves" rule as
  // generateContentIdeas/runPipelineFor applies here too. Returns the
  // accepted entries so the caller can show a real count, not just "done."
  async function suggestTransitionPlan(candidates, forProjectId = currentProjectId) {
    if (!forProjectId || candidates.length === 0) return [];
    const targetSlice = getState().projects[forProjectId];
    const wordsToUse = targetSlice?.words ?? [];
    if (wordsToUse.length === 0) return [];
    upsertProjectJob(forProjectId, "transitionPlan", { running: true, error: "" });
    try {
      const result = await invoke("suggest_transition_plan", {
        words: wordsToUse,
        speakers: targetSlice?.speakers ?? [],
        candidates: candidates.map((c) => ({ time: c.time })),
        language: targetSlice?.detectedLanguage || null,
      });
      if (result.length > 0) {
        const current = getState().projects[forProjectId]?.videoTransitions || [];
        upsertProjectSlice(forProjectId, {
          videoTransitions: [...current, ...result.map((entry) => ({ time: entry.time, effect: entry.effect }))],
        });
        flushProjectSave(forProjectId);
      }
      return result;
    } catch (err) {
      upsertProjectJob(forProjectId, "transitionPlan", { error: String(err) });
      throw err;
    } finally {
      upsertProjectJob(forProjectId, "transitionPlan", { running: false });
    }
  }

  // "Record from mic" without the "also use as voice-over" checkbox
  // (VoiceoverSection.jsx) -- replaces the transcript from freshly
  // recorded speech without touching the video's own audio or an already
  // active voiceover, unlike handleVoiceoverReady.
  //
  // `forProjectId` defaults to whatever's current at call time -- the
  // global-hotkey dictation HUD (see the `dictation-result` listener
  // above) has no notion of "which project this was for," and by design
  // always targets whichever project is open when its result arrives
  // (that window doesn't even know a video is loaded, per its own doc
  // comment). VoiceoverSection.jsx's own "record from mic, captions only"
  // path, by contrast, DOES have a real originating project and passes it
  // explicitly.
  function handleCaptionsFromRecording(words, forProjectId = currentProjectId) {
    if (!forProjectId) return;
    upsertProjectSlice(forProjectId, {
      words: words || [],
      prosody: [],
      speakers: [],
      // Same staleness reasoning as elsewhere -- burned output and
      // generated title/hashtags were written against the old transcript.
      lastBurnedPath: "",
      contentIdeas: null,
      contentStrategyOptions: [],
    });
    upsertProjectJob(forProjectId, "prosody", { status: "" });
    upsertProjectJob(forProjectId, "diarize", { status: "" });
    upsertProjectJob(forProjectId, "contentIdeas", { error: "" });
  }

  // Same shape as `handleVoiceoverReady`/`handleMusicBedChange` above --
  // `forProjectId` is captured by SilenceRemovalPanel.jsx at the start of
  // its (slow: ffmpeg re-encode) call.
  function handleJumpCutApplied(result, forProjectId) {
    upsertProjectSlice(forProjectId, {
      // Video preview picks up the new file automatically via the videoPath prop.
      videoPath: result.output_path,
      words: result.words,
      // Any active voiceover's sync offset was computed against the
      // *pre-cut* video's timing — re-cutting invalidates it, the same
      // staleness class handleVoiceoverReady guards prosody/speakers
      // against.
      voiceoverPath: "",
      voiceoverOffset: 0,
      // Same staleness reasoning -- a music bed's ducking windows were
      // computed against the pre-cut transcript's timing too.
      musicPath: "",
      // The last burned file was rendered from words/audio that no longer
      // match this freshly re-cut video -- scheduling it to Instagram now
      // would post stale output.
      lastBurnedPath: "",
      // Same staleness reasoning -- generated title/hashtags were written
      // against the pre-cut transcript.
      contentIdeas: null,
      contentStrategyOptions: [],
      // Same staleness reasoning, made more visible than the others: a
      // re-cut renumbers the whole timeline, so a stale [start,end)
      // override range wouldn't just be slightly off, it would apply the
      // wrong theme to entirely the wrong words.
      captionStyleOverrides: [],
      // Same staleness reasoning again -- a transition's absolute `time`
      // would land on whatever now happens to sit at that timestamp in
      // the re-cut video, not the moment it was actually placed for.
      videoTransitions: [],
    });
    upsertProjectJob(forProjectId, "contentIdeas", { error: "" });
    if (forProjectId === currentProjectId) {
      setCurrentTime(0);
      setDuration(0);
    }
  }

  async function burnCaptions() {
    if (!videoPath || words.length === 0 || !currentProjectId) return;
    const forProjectId = currentProjectId;

    // No client-side freshness check here — burn_captions itself verifies
    // (via a disk-persisted record, not React state) that videoPath hasn't
    // changed since it was transcribed, and rejects with a clear error if
    // it has. That holds even across separate app sessions/processes,
    // which an in-memory check here would not.

    // Always lands inside this project's own media/<id>/processed/ folder
    // (library.rs) instead of wherever a save dialog pointed -- so a
    // burned export is always somewhere the app itself can find and load
    // back, per the project's own goal. burn_captions itself is otherwise
    // completely unchanged; this is just a different source for the same
    // `outputPath` argument it always took.
    const outputPath = await invoke("processed_output_path", { projectId: forProjectId });

    upsertProjectJob(forProjectId, "burn", { running: true, status: "", progress: null });
    const startedAt = Date.now();
    const unlisten = await listen("burn-progress", (event) => {
      if (event.payload?.project_id === forProjectId) upsertProjectJob(forProjectId, "burn", { progress: event.payload });
    });
    try {
      const result = await invoke("burn_captions", {
        videoPath,
        words,
        style: captionStyle,
        outputPath,
        prosody,
        speakers,
        overrides: captionStyleOverrides,
        videoTransitions: videoTransitions.map((t) => ({ time: t.time, effect: t.effect })),
        voiceoverPath: voiceoverPath || null,
        voiceoverOffsetSeconds: voiceoverPath ? voiceoverOffset : null,
        projectId: forProjectId,
      });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      // Writes into `forProjectId`'s own slice directly, correct whether
      // or not it's still the displayed project -- there's no longer a
      // shared mutable location for two projects' results to collide on,
      // replacing the old two-path "current vs. not" special case
      // entirely (see projectStore.js's own doc comment on why routing by
      // id removes the need for that distinction).
      upsertProjectSlice(forProjectId, { lastBurnedPath: result });
      // Saved *immediately*, not just via the store's own debounced
      // autosave -- this is the one guarantee "Save" (Burn & Export) is
      // actually for: reopening this project later should reliably resume
      // from at least this exact checkpoint, even if the app closed
      // within the debounce's ~1s window.
      flushProjectSave(forProjectId);
      upsertProjectJob(forProjectId, "burn", { status: `Saved captioned video to the processed folder in ${elapsed}.` });
    } catch (err) {
      upsertProjectJob(forProjectId, "burn", { status: `Error: ${err}` });
    } finally {
      unlisten();
      upsertProjectJob(forProjectId, "burn", { running: false, progress: null });
    }
  }

  // Reveals the actual burned file in the OS file manager -- otherwise
  // the app-managed processed/ folder has no obvious way to find without
  // knowing where app_data_dir() lives.
  function revealLastBurned() {
    if (lastBurnedPath) invoke("reveal_in_folder", { path: lastBurnedPath }).catch(() => {});
  }

  if (authLoading) {
    return (
      <div className="container auth-screen">
        <p className="subtitle">Loading…</p>
      </div>
    );
  }

  if (!user) {
    return <AuthScreen />;
  }

  if (missingCore === null) {
    return (
      <div className="container auth-screen">
        <p className="subtitle">Loading…</p>
      </div>
    );
  }

  if (missingCore.length > 0) {
    return <RuntimeSetup components={missingCore} onReady={() => setMissingCore([])} />;
  }

  return (
    <AppShell
      videoPath={videoPath}
      originalVideoPath={originalVideoPath}
      videoRef={videoRef}
      currentTime={currentTime}
      duration={duration}
      setCurrentTime={setCurrentTime}
      setDuration={setDuration}
      words={words}
      pipelineStatus={noProjectStatus || pipelineStatus}
      pipelineRunning={pipelineRunning}
      pipelineProgress={pipelineProgress}
      detectedLanguage={detectedLanguage}
      captionStyle={captionStyle}
      setCaptionStyle={setCaptionStyle}
      captionThemeId={captionThemeId}
      setCaptionThemeId={setCaptionThemeId}
      resetCaptionStyleToFactoryDefault={resetCaptionStyleToFactoryDefault}
      captionStyleOverrides={captionStyleOverrides}
      addCaptionStyleOverride={addCaptionStyleOverride}
      removeCaptionStyleOverride={removeCaptionStyleOverride}
      updateCaptionStyleOverride={updateCaptionStyleOverride}
      videoTransitions={videoTransitions}
      addVideoTransition={addVideoTransition}
      updateVideoTransition={updateVideoTransition}
      removeVideoTransition={removeVideoTransition}
      suggestTransitionPlan={suggestTransitionPlan}
      suggestingTransitionPlan={suggestingTransitionPlan}
      transitionPlanError={transitionPlanError}
      burnStatus={burnStatus}
      burning={burning}
      burnProgress={burnProgress}
      prosody={prosody}
      analyzingProsody={analyzingProsody}
      prosodyStatus={prosodyStatus}
      prosodyProgress={prosodyProgress}
      analyzeProsody={analyzeProsody}
      speakers={speakers}
      analyzingSpeakers={analyzingSpeakers}
      diarizeStatus={diarizeStatus}
      diarizeProgress={diarizeProgress}
      diarizeSpeakers={diarizeSpeakers}
      voiceoverPath={voiceoverPath}
      voiceoverOffset={voiceoverOffset}
      onVoiceoverReady={handleVoiceoverReady}
      musicPath={musicPath}
      duckLevel={duckLevel}
      onMusicBedChange={handleMusicBedChange}
      onCaptionsFromRecording={handleCaptionsFromRecording}
      contentIdeas={contentIdeas}
      contentStrategyOptions={contentStrategyOptions}
      contentHints={contentHints}
      setContentHints={setContentHints}
      selectContentStrategyOption={selectContentStrategyOption}
      generatingContentIdeas={generatingContentIdeas}
      contentIdeasError={contentIdeasError}
      generateContentIdeas={generateContentIdeas}
      pickVideo={pickVideo}
      importing={importing}
      importProgress={importProgress}
      currentProjectId={currentProjectId}
      loadProject={loadProject}
      deleteCurrentProject={deleteCurrentProject}
      retranscribe={retranscribe}
      projectTitle={projectTitle}
      setProjectTitle={setProjectTitle}
      projectDescription={projectDescription}
      setProjectDescription={setProjectDescription}
      projectHashtags={projectHashtags}
      setProjectHashtags={setProjectHashtags}
      handleSeek={handleSeek}
      handleWordChange={handleWordChange}
      handleWordDelete={handleWordDelete}
      handleJumpCutApplied={handleJumpCutApplied}
      burnCaptions={burnCaptions}
      revealLastBurned={revealLastBurned}
      lastBurnedPath={lastBurnedPath}
    />
  );
}

export default App;
