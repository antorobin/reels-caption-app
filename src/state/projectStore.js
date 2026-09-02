import { useSyncExternalStore } from "react";

// The per-project state store that replaces App.jsx's old single flat
// `useState`-per-field model. Module-level (not React state) so a
// project's data survives regardless of which project is currently
// *displayed* -- `currentProjectId` only ever selects which slice the UI
// shows; writing into a different project's slice (a background pipeline
// finishing after the user switched away) is always valid and never
// touches whatever's on screen.
//
// This exists because of a real, reproduced bug fixed earlier this
// session: with one shared blob of state, a slow operation (content-
// strategy generation, voiceover synthesis, MusicGen, ...) that finished
// *after* the user switched to a different project silently overwrote
// that *different* project's data. The first fix was a guard
// (`projectIdRef`) that discarded a stale result rather than misapplying
// it -- safe, but it also meant a project's work simply stopped the
// moment you looked away from it. Routing by id here removes the need for
// that guard entirely: there's no longer a shared mutable location for
// two projects' results to collide on, so "is this still the open
// project" is no longer a question a completion handler has to get right.
//
// Selector hooks are built on React 18's built-in `useSyncExternalStore`
// (no new dependency) specifically so a background project's updates
// don't re-render whatever project is actually displayed -- each hook
// only re-renders when the exact slice/field it reads changes.

const listeners = new Set();

let state = {
  currentProjectId: "",
  projects: {}, // Record<projectId, ProjectSlice>
};

function emitChange() {
  for (const listener of listeners) listener();
}

export function subscribe(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getState() {
  return state;
}

/// A brand-new/never-loaded project's slice -- every field
/// `applyProjectState` used to set directly, now namespaced per id.
/// `captionStyleDefault` is passed in by the caller (App.jsx) rather than
/// imported here, so this module doesn't need to know about
/// CaptionStyleEditor.jsx's default shape.
export function emptyProjectSlice(id, captionStyleDefault) {
  return {
    id,
    createdAt: 0,
    originalFilename: "",
    title: "",
    description: "",
    hashtags: [],
    videoPath: "",
    originalVideoPath: "",
    lastBurnedPath: "",
    words: [],
    detectedLanguage: null,
    captionStyle: captionStyleDefault,
    // Which CAPTION_THEMES entry `captionStyle` was last picked from --
    // NOT updated on a granular tweak, only when an actual theme card is
    // clicked. Comparing the current `captionStyle` against this theme's
    // own stock style (CaptionStyleEditor.jsx) is what lets the UI show
    // "<theme name>" vs. "<theme name> (Custom)" without a separate
    // stored flag that could drift out of sync with the style itself.
    captionThemeId: "cascade-bold",
    // A different CaptionStyle applied to just one time range of the
    // video, layered on top of `captionStyle` -- [{start, end, style,
    // themeId}], enforced non-overlapping by the UI at creation time
    // (CaptionOverridePanel.jsx), never arbitrated here. `themeId` is
    // display-only (which theme this override's style was seeded from,
    // same convention as the top-level `captionThemeId` above); the burn
    // pipeline only ever reads `start`/`end`/`style`.
    captionStyleOverrides: [],
    prosody: [],
    speakers: [],
    voiceoverPath: "",
    voiceoverOffset: 0,
    musicPath: "",
    duckLevel: 0.3,
    contentIdeas: null,
    contentStrategyOptions: [],
    contentHints: "",
    // Transient, per-project job status -- never persisted to
    // library.state (a "still transcribing" flag has no meaning after a
    // restart), only ever held here in memory. Lets a project's own
    // status text/progress bar be correct if the user switches back to it
    // mid-job, instead of the flat App.jsx-level status strings that used
    // to apply to "whichever project is currently displayed" regardless
    // of which project's job actually produced them.
    jobs: {
      pipeline: { running: false, status: "", progress: null },
      prosody: { running: false, status: "", progress: null },
      diarize: { running: false, status: "", progress: null },
      burn: { running: false, status: "", progress: null },
      contentIdeas: { running: false, error: "" },
    },
  };
}

/// Sets `currentProjectId` -- the *only* thing that changes which
/// project's slice the UI displays. Never clears or mutates any project's
/// slice by itself.
export function setCurrentProjectId(id) {
  if (state.currentProjectId === id) return;
  state = { ...state, currentProjectId: id };
  emitChange();
}

/// Creates `id`'s slice if it doesn't exist yet (a no-op otherwise) --
/// used when opening/importing a project, before any `upsertProjectSlice`
/// call needs somewhere to merge into.
export function ensureProjectSlice(id, initial) {
  if (!id || state.projects[id]) return;
  state = { ...state, projects: { ...state.projects, [id]: initial } };
  emitChange();
}

/// Replaces `id`'s entire slice outright (vs. merging) -- used when
/// (re-)hydrating a project from a fresh `get_project`/`create_project`
/// result, where the incoming object is already a complete slice, not a
/// partial update.
export function setProjectSlice(id, slice) {
  if (!id) return;
  state = { ...state, projects: { ...state.projects, [id]: slice } };
  emitChange();
  maybeScheduleAutosave(id);
}

/// Merges `patch` (a plain object, or a function `(prevSlice) => partial`
/// for updates that need to read the previous value) into `id`'s slice.
/// This is the one function every background operation writes through --
/// always addressed by the id it was actually started for, never by
/// "whichever project is current," which is the whole point of this
/// store existing. If `id`'s slice doesn't exist yet (shouldn't normally
/// happen -- a project is always `ensureProjectSlice`d on open/import
/// first) a bare id-only slice is created rather than silently dropping
/// the write.
export function upsertProjectSlice(id, patch) {
  if (!id) return;
  const prev = state.projects[id] ?? { id };
  const partial = typeof patch === "function" ? patch(prev) : patch;
  const next = { ...prev, ...partial };
  state = { ...state, projects: { ...state.projects, [id]: next } };
  emitChange();
  maybeScheduleAutosave(id);
  if (touchesReembedFields(partial)) maybeScheduleReembed(id);
}

/// Merges a patch into one project's `jobs` sub-object specifically --
/// the common case for progress/status updates, so call sites don't all
/// have to hand-spread `jobs` themselves.
export function upsertProjectJob(id, jobName, patch) {
  upsertProjectSlice(id, (prev) => ({
    jobs: { ...prev.jobs, [jobName]: { ...prev.jobs?.[jobName], ...patch } },
  }));
}

export function removeProjectSlice(id) {
  if (!state.projects[id]) return;
  const rest = { ...state.projects };
  delete rest[id];
  state = { ...state, projects: rest };
  emitChange();
}

// ---- per-project debounced autosave / re-embed ----
//
// One timer per project id (not one shared timer for "whichever project
// is open") -- a background project's own updates reschedule *its own*
// save, independent of whatever the displayed project is doing. Lives
// here rather than in a React `useEffect` because there is no longer one
// "current project" a single effect could key off of; any number of
// projects can be actively mutating their own slices at once.

/// Registered once by App.jsx (it owns `invoke` and knows the exact shape
/// `save_project` expects) so every `upsertProjectSlice`/`setProjectSlice`
/// call below can trigger autosave/reembed *automatically* -- callers never
/// have to remember to schedule a save themselves, which would be an easy
/// thing to silently forget at any one of the many call sites this store
/// is written from (background jobs, synchronous UI edits, project
/// load/create). `buildPayload(slice)` turns a slice into `save_project`'s
/// expected shape (App.jsx's `projectPayloadFrom`).
let autosaveConfig = null;
export function configureAutosave(invokeFn, buildPayload) {
  autosaveConfig = { invokeFn, buildPayload };
}

const saveTimers = new Map();
const AUTOSAVE_DEBOUNCE_MS = 1000;

function maybeScheduleAutosave(id) {
  if (!autosaveConfig || !id) return;
  const existing = saveTimers.get(id);
  if (existing) clearTimeout(existing);
  const timer = setTimeout(() => {
    saveTimers.delete(id);
    const slice = state.projects[id];
    if (!slice) return;
    autosaveConfig.invokeFn("save_project", { project: autosaveConfig.buildPayload(slice) }).catch(() => {});
  }, AUTOSAVE_DEBOUNCE_MS);
  saveTimers.set(id, timer);
}

/// Saves `id`'s current slice *immediately*, bypassing the debounce
/// entirely -- for the specific, real gap the debounce alone leaves open:
/// closing the app within the 1s window right after a meaningful step
/// finishes (transcription, content-strategy generation, prosody/
/// diarization, a burn) means that `setTimeout` never fires at all (the
/// whole process is gone), silently losing whatever just completed. Every
/// call site that finishes one of those steps calls this right away
/// instead of just letting `upsertProjectSlice`'s automatic debounced
/// save handle it. Also cancels any pending debounced timer for `id`, so
/// a redundant save doesn't also fire a moment later.
export function flushProjectSave(id) {
  if (!autosaveConfig || !id) return Promise.resolve();
  const existing = saveTimers.get(id);
  if (existing) {
    clearTimeout(existing);
    saveTimers.delete(id);
  }
  const slice = state.projects[id];
  if (!slice) return Promise.resolve();
  return autosaveConfig.invokeFn("save_project", { project: autosaveConfig.buildPayload(slice) }).catch(() => {});
}

const reembedTimers = new Map();
const REEMBED_DEBOUNCE_MS = 8000;
// Matches the original App.jsx effect's own dependency list exactly --
// only these fields (what `library.rs`'s `embeddable_text` actually
// reads title/description/hashtags from directly) trigger a re-embed. A
// transcript-only edit still gets picked up indirectly, the same way it
// always did: a fresh transcript flows into generated content ideas,
// which flows into title/description via the seeding effect, which
// itself touches one of these fields.
const REEMBED_TRIGGER_FIELDS = ["title", "description", "hashtags"];

function touchesReembedFields(partial) {
  return partial && typeof partial === "object" && REEMBED_TRIGGER_FIELDS.some((field) => field in partial);
}

function maybeScheduleReembed(id) {
  if (!autosaveConfig || !id) return;
  const existing = reembedTimers.get(id);
  if (existing) clearTimeout(existing);
  const timer = setTimeout(() => {
    reembedTimers.delete(id);
    autosaveConfig.invokeFn("reembed_project", { id }).catch(() => {});
  }, REEMBED_DEBOUNCE_MS);
  reembedTimers.set(id, timer);
}

/// Cancels any pending autosave/reembed for `id` -- called when a project
/// is deleted, so a stale timer doesn't try to save/reembed a project that
/// no longer exists a second later.
export function cancelProjectTimers(id) {
  const saveTimer = saveTimers.get(id);
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimers.delete(id);
  }
  const reembedTimer = reembedTimers.get(id);
  if (reembedTimer) {
    clearTimeout(reembedTimer);
    reembedTimers.delete(id);
  }
}

// ---- selector hooks ----

export function useCurrentProjectId() {
  return useSyncExternalStore(subscribe, () => getState().currentProjectId);
}

/// `undefined` when `id` is falsy or has no slice yet -- callers fall back
/// to a sensible empty/default shape themselves (App.jsx does this once,
/// centrally, rather than every selector needing its own default).
export function useProjectSlice(id) {
  return useSyncExternalStore(subscribe, () => (id ? getState().projects[id] : undefined));
}

/// Ids of every project with at least one job currently `running: true` --
/// what Sidebar.jsx's per-row badges and the "N processing" summary strip
/// (Phase 3) are built on. Deliberately computed from the store's own
/// `jobs` state, not from `list_projects` (which only ever reflects disk
/// state, never "is a background job in flight right now").
///
/// `useSyncExternalStore` requires a snapshot function that returns a
/// *referentially stable* value when nothing the caller cares about has
/// changed -- naively recomputing a fresh array on every call would return
/// a new reference on every single progress-percent tick (each one
/// changes `state`), even though the actual *set of processing ids*
/// usually hasn't changed, causing needless re-renders of every
/// subscriber (in the worst case, an infinite re-render loop). The cache
/// below recomputes only when `state` has actually changed since last
/// call, and even then only replaces the returned array if the computed
/// set of ids is genuinely different from last time.
let processingIdsCache = { forState: null, value: [] };

export function useAnyProjectProcessing() {
  return useSyncExternalStore(subscribe, () => {
    if (processingIdsCache.forState === state) return processingIdsCache.value;

    const ids = Object.entries(state.projects)
      .filter(([, slice]) => Object.values(slice.jobs || {}).some((job) => job && job.running))
      .map(([id]) => id)
      .sort();

    const prev = processingIdsCache.value;
    const unchanged = ids.length === prev.length && ids.every((id, i) => id === prev[i]);
    processingIdsCache = { forState: state, value: unchanged ? prev : ids };
    return processingIdsCache.value;
  });
}
