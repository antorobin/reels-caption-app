import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import ProgressBar from "../ProgressBar.jsx";
import { useAnyProjectProcessing } from "../../state/projectStore.js";

const PAGE_SIZE = 8;

// Same shape as ScheduleToInstagramButton.jsx's own formatWhen -- renders
// in whatever timezone the OS/browser is already set to, no separate
// timezone handling needed.
function formatDateTime(unixSeconds) {
  return new Date(unixSeconds * 1000).toLocaleString();
}

// `<input type="date">` gives back a plain "YYYY-MM-DD" string with no
// time/timezone component -- parsed as local midnight (not UTC midnight)
// so a project uploaded at any time on that calendar day, in this
// machine's own timezone, counts as "on" that date, matching how
// `formatDateTime` above already displays everything in local time.
function startOfDayTimestamp(dateStr) {
  if (!dateStr) return null;
  return Math.floor(new Date(`${dateStr}T00:00:00`).getTime() / 1000);
}
function endOfDayTimestamp(dateStr) {
  if (!dateStr) return null;
  return Math.floor(new Date(`${dateStr}T23:59:59.999`).getTime() / 1000);
}

// Media Pool + the persistent project library (library.rs). Self-fetches
// its own list via `list_projects` -- matching ScheduleToInstagramButton.jsx's
// own self-fetching precedent -- rather than lifting the list into App.jsx,
// since fetching/refetching/paginating is entirely this component's own
// concern.
//
// Title/description/hashtags auto-fill from the AI-generated content ideas
// the moment transcription finishes (App.jsx's runPipelineFor fires
// generateContentIdeas in the background as soon as there's a real
// transcript to work from -- no manual click needed for a fresh import
// with audio), but stay fully inline-editable here either way: typing in
// any of the three fields below is what App.jsx's autosave effect
// persists, and the seeding effect only ever fills them in while the
// title still reads as the placeholder, so it can never clobber an edit.
function Sidebar({
  videoPath,
  originalVideoPath,
  onPickVideo,
  importing,
  importProgress,
  currentProjectId,
  onSelectProject,
  projectTitle,
  setProjectTitle,
  projectDescription,
  setProjectDescription,
  projectHashtags,
  setProjectHashtags,
  generatingContentIdeas,
}) {
  const [projects, setProjects] = useState([]);
  const [page, setPage] = useState(0);
  const [loadError, setLoadError] = useState("");
  // Ids of every project with a background job actually running right now
  // -- from the store's own `jobs` state (see projectStore.js's own doc
  // comment on why this can't come from `list_projects`, which only ever
  // reflects disk state). Drives the per-row spinner below.
  const processingIds = useAnyProjectProcessing();
  // Semantic (cosine-similarity, vector-embedding) search over the library
  // -- see library.rs's `search_projects` doc comment for how this actually
  // ranks. `searchResults` is `null` while no search is active (renders the
  // normal paginated/sorted list); an empty array is a real "no matches",
  // distinct from "haven't searched yet". Debounced client-side rather than
  // firing `search_projects` on every keystroke -- each call embeds the
  // query text through the same local model server embeddings.rs manages,
  // and while that's fast once warm, there's no reason to fire it for every
  // half-typed word.
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  // Date-range filter, combinable with the keyword search above (applied
  // client-side on top of whichever list -- ranked search results or the
  // full library -- is currently in play; date range isn't something
  // `search_projects`'s embedding-based ranking knows anything about).
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  // Requested directly: a way to view/hear the untouched original import
  // for reference, since the main preview's own video can be a jump-cut
  // re-render (a different file from the original) and its audio track
  // mutes once a voiceover is active -- neither of those touch
  // `originalVideoPath`, which App.jsx sets once per project and never
  // mutates. Lazily mounted (only rendered while toggled open) rather than
  // always-present, since it's a reference view, not the main editing one.
  const [showOriginal, setShowOriginal] = useState(false);

  async function refresh() {
    try {
      const list = await invoke("list_projects");
      setProjects(list);
      setLoadError("");
    } catch (err) {
      setLoadError(String(err));
    }
  }

  // Refetch once on mount, and again right after a new import finishes --
  // `importing` flips false->true->false around create_project, so this
  // catches the moment a brand-new project needs to appear in the list.
  useEffect(() => {
    refresh();
  }, []);
  useEffect(() => {
    if (!importing) refresh();
  }, [importing]);
  // Closes the original-video viewer on a project switch -- it's an
  // explicit, per-project reference view, not something that should stay
  // open (and start playing a different video) just because you clicked a
  // different row in the list.
  useEffect(() => {
    setShowOriginal(false);
  }, [currentProjectId]);

  async function handlePickVideo() {
    await onPickVideo();
    setPage(0);
  }

  // Debounced 400ms after the last keystroke. A cleared query just drops
  // back to the normal list (searchResults -> null) with no backend call.
  useEffect(() => {
    const query = searchQuery.trim();
    if (!query) {
      setSearchResults(null);
      setSearching(false);
      setSearchError("");
      return undefined;
    }
    setSearching(true);
    setSearchError("");
    const timer = setTimeout(async () => {
      try {
        const results = await invoke("search_projects", { query });
        setSearchResults(results);
      } catch (err) {
        setSearchError(String(err));
        setSearchResults([]);
      } finally {
        setSearching(false);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  useEffect(() => {
    setPage(0);
  }, [searchQuery, dateFrom, dateTo]);

  // Sorted by created_at (newest import first) -- deliberately not
  // updated_at, which would make the project currently being edited jump
  // to the top of its own list on every autosave tick. list_projects
  // already returns rows in this order, but re-sorting here is cheap
  // insurance if that ever changes.
  const sorted = [...projects].sort((a, b) => b.created_at - a.created_at);

  const hasQuery = searchQuery.trim().length > 0;
  const hasDateFilter = !!dateFrom || !!dateTo;
  const isFiltering = hasQuery || hasDateFilter;
  const fromTs = startOfDayTimestamp(dateFrom);
  const toTs = endOfDayTimestamp(dateTo);
  function withinDateRange(p) {
    if (fromTs !== null && p.created_at < fromTs) return false;
    if (toTs !== null && p.created_at > toTs) return false;
    return true;
  }
  // A keyword search already comes back ranked by similarity (nearest
  // first) -- that order is the whole point, so a date filter on top of it
  // only narrows the set, never re-sorts it. While a query is set but its
  // debounced result hasn't landed yet, this is `[]` rather than the full
  // list -- searching must never show "the entire listing" in the
  // meantime, only its own eventual results (or nothing, while pending).
  const baseList = hasQuery ? searchResults ?? [] : sorted;
  const displayed = isFiltering ? baseList.filter(withinDateRange) : sorted;
  const totalPages = Math.max(1, Math.ceil(displayed.length / PAGE_SIZE));
  const currentPage = Math.min(page, totalPages - 1);
  const pageItems = displayed.slice(currentPage * PAGE_SIZE, currentPage * PAGE_SIZE + PAGE_SIZE);

  return (
    <aside className="shell-sidebar">
      <div className="shell-sidebar-heading">Media Pool</div>
      <div className="shell-sidebar-body">
        {videoPath ? (
          <div className="shell-sidebar-thumb">
            {showOriginal && originalVideoPath ? (
              <video controls autoPlay src={convertFileSrc(originalVideoPath)} className="shell-sidebar-original-player" />
            ) : (
              <div
                className="shell-sidebar-thumb-frame"
                onClick={() => originalVideoPath && setShowOriginal(true)}
                role={originalVideoPath ? "button" : undefined}
                title={originalVideoPath ? "View the original, untouched video" : undefined}
              >
                <svg width="22" height="22" viewBox="0 0 24 24" fill="var(--shell-text-dim)">
                  <path d="M8 5v14l11-7z" />
                </svg>
              </div>
            )}
            <div className="shell-sidebar-video-path">{videoPath.split(/[\\/]/).pop()}</div>
            {originalVideoPath && (
              <button type="button" className="link-button shell-sidebar-original-toggle" onClick={() => setShowOriginal((v) => !v)}>
                {showOriginal ? "Hide original video" : "▶ View original video"}
              </button>
            )}
          </div>
        ) : (
          <p className="shell-viewer-empty">No video loaded yet.</p>
        )}
        <button onClick={handlePickVideo} disabled={importing}>
          {importing ? "Importing…" : videoPath ? "Choose more videos…" : "Choose video(s)…"}
        </button>
        {importing && <ProgressBar progress={importProgress} />}

        {currentProjectId && (
          <div className="project-details-form">
            {generatingContentIdeas && (
              <p className="shell-sidebar-hint">✨ Generating title, description & hashtags from the transcript…</p>
            )}
            <input
              className="project-title-input"
              value={projectTitle}
              onChange={(e) => setProjectTitle(e.target.value)}
              placeholder="Title"
            />
            <textarea
              className="project-description-input"
              value={projectDescription}
              onChange={(e) => setProjectDescription(e.target.value)}
              placeholder="Description"
              rows={2}
            />
            <input
              className="project-hashtags-input"
              value={projectHashtags.join(" ")}
              onChange={(e) => setProjectHashtags(e.target.value.split(/\s+/).filter(Boolean))}
              placeholder="#hashtags #here"
            />
          </div>
        )}

        <div className="shell-sidebar-heading project-list-heading">
          Projects{sorted.length > 0 && <span className="project-list-count"> ({sorted.length})</span>}
        </div>
        <input
          type="search"
          className="project-search-input"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search by meaning, not just title…"
        />
        <div className="project-date-filter">
          <label>
            From
            <input type="date" value={dateFrom} max={dateTo || undefined} onChange={(e) => setDateFrom(e.target.value)} />
          </label>
          <label>
            To
            <input type="date" value={dateTo} min={dateFrom || undefined} onChange={(e) => setDateTo(e.target.value)} />
          </label>
          {hasDateFilter && (
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setDateFrom("");
                setDateTo("");
              }}
            >
              Clear dates
            </button>
          )}
        </div>
        {searching && <p className="shell-sidebar-hint">Searching…</p>}
        {searchError && <pre className="result">{searchError}</pre>}
        {loadError && <pre className="result">{loadError}</pre>}
        <div className="project-list">
          {pageItems.map((p) => (
            <div
              key={p.id}
              className={p.id === currentProjectId ? "project-list-item active" : "project-list-item"}
              onClick={() => onSelectProject(p)}
            >
              <div className="project-list-title">
                {processingIds.includes(p.id) && (
                  <span className="project-processing-badge" title="Processing in the background…" />
                )}
                {p.title || p.original_filename}
              </div>
              {p.description && <div className="project-list-description">{p.description}</div>}
              {p.hashtags.length > 0 && <div className="project-list-hashtags">{p.hashtags.join(" ")}</div>}
              <div className="project-list-filename">{p.original_filename}</div>
              <div className="project-list-dates">
                <span>Uploaded {formatDateTime(p.created_at)}</span>
                <span>Updated {formatDateTime(p.updated_at)}</span>
              </div>
            </div>
          ))}
          {pageItems.length === 0 && !loadError && !searching && (
            <p className="shell-viewer-empty">{isFiltering ? "No results found." : "No projects yet."}</p>
          )}
        </div>
        {totalPages > 1 && (
          <div className="project-list-pagination">
            <button type="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>
              ‹
            </button>
            <span>
              {currentPage + 1} / {totalPages}
            </span>
            <button type="button" disabled={currentPage >= totalPages - 1} onClick={() => setPage(currentPage + 1)}>
              ›
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}

export default Sidebar;
