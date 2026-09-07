// Derives a project's display status entirely on the frontend, never in
// library.rs -- `Project.state` is deliberately opaque to Rust (see
// library.rs's own module doc comment: `CaptionStyle` is Deserialize-only,
// and Rust never needs to understand `state`'s shape, only round-trip it),
// so anything that needs to peek inside it (like "does this project have a
// transcript yet?") belongs here, on the same side that already assembles
// and reads that blob.
//
// Seven statuses, checked in priority order (a project can technically
// satisfy more than one -- e.g. "burned" and "exported" -- so this always
// reports the most *advanced* one that's true, not the first one
// historically reached):
// - "processing": a background job is actually running right now for this
//   project (from projectStore.js's own live, in-memory `jobs` state --
//   see useAnyProjectProcessing's doc comment for why this can't come
//   from the persisted `Project` row at all). Always wins over every
//   other status below, live state trumping a stale snapshot.
// - "drafted": `original_path` is empty -- an AI content-strategy draft
//   (library.rs's `create_strategy_draft`) with no real video yet. Checked
//   right after "processing" and before every status below, since a
//   draft can't have a real transcript/burn/export/publish regardless of
//   what else its row says -- there's no video for any of those to have
//   happened to yet.
// - "published": `published_at` is set -- a real post via
//   post_to_instagram_now succeeded for this project (library.rs's
//   `mark_project_published`). NOTE a real, open gap: a *scheduled*
//   (not-yet-fired) Instagram post never reaches this, since
//   scheduler.rs's own queue has no notion of which project a scheduled
//   job came from -- see ScheduleToInstagramButton.jsx's own comment on
//   its "now" vs. scheduled branches.
// - "exported": `exported_at` is set -- the user has actually taken the
//   burned output out of the app at least once (library.rs's
//   `mark_project_exported`), distinct from merely having burned one.
// - "burned": `last_burned_path` is set -- captions have been burned at
//   least once, whether or not the result was ever exported/published.
// - "transcribed": `state.words` is a non-empty array -- transcription
//   has produced real word timestamps, but nothing has been burned yet.
// - "pending": none of the above -- a freshly imported project with a
//   real video but no transcript yet.
export const PROJECT_STATUSES = {
  drafted: { label: "Drafted", icon: "📋" },
  pending: { label: "Pending", icon: "⏳" },
  processing: { label: "Processing", icon: "⚙️" },
  transcribed: { label: "Transcribed", icon: "📝" },
  burned: { label: "Burned", icon: "🔥" },
  exported: { label: "Exported", icon: "📤" },
  published: { label: "Published", icon: "✅" },
};

export function deriveProjectStatus(project, isProcessing) {
  if (isProcessing) return "processing";
  if (!project.original_path) return "drafted";
  if (project.published_at) return "published";
  if (project.exported_at) return "exported";
  if (project.last_burned_path) return "burned";
  const words = project.state?.words;
  if (Array.isArray(words) && words.length > 0) return "transcribed";
  return "pending";
}

// Whether a project is an AI content-strategy draft (no video yet) --
// used by Sidebar.jsx to decide what clicking a row / its pencil icon
// should actually do (reopen the strategy form, not the normal
// title/description edit modal or project-load flow).
export function isStrategyDraft(project) {
  return !project.original_path;
}
