import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import EditProjectModal from "./EditProjectModal.jsx";
import NewProjectModal from "./NewProjectModal.jsx";
import VideoPreviewModal from "./VideoPreviewModal.jsx";
import { useAnyProjectProcessing } from "../../state/projectStore.js";
import { deriveProjectStatus, isStrategyDraft, PROJECT_STATUSES } from "../../lib/projectStatus.js";

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

// A small status icon+label, reused for every row -- see projectStatus.js
// for what each status actually means and how it's derived.
function StatusBadge({ statusKey }) {
  const status = PROJECT_STATUSES[statusKey];
  if (!status) return null;
  return (
    <span className={`project-status-badge status-${statusKey}`} title={status.label}>
      {status.icon} {status.label}
    </span>
  );
}

// The persistent project library (library.rs). Self-fetches its own list
// via `list_projects` -- matching ScheduleToInstagramButton.jsx's own
// self-fetching precedent -- rather than lifting the list into App.jsx,
// since fetching/refetching/paginating is entirely this component's own
// concern.
//
// Requested directly, in two passes now: (1) editing moved behind a
// pencil icon instead of permanently-visible inline inputs, and (2) the
// whole standalone "Media Pool" section (video thumbnail, "Choose
// video(s)…" button) is gone entirely, replaced by a single "+" button
// next to the "Projects" heading that opens `NewProjectModal.jsx` --
// upload videos or start an AI content-strategy draft. Every row now
// carries its own ✏️ edit / 🎬 view-original actions (previously only the
// currently-open project had these, and only via a separate header
// area this component no longer has). There's no separate "currently
// open project" display left at all -- the active row's own highlight
// (`.project-list-item.active`, unchanged) is the only "what's open"
// indicator now.
function Sidebar({
  currentProjectId,
  onSelectProject,
  onPickVideo,
  projectTitle,
  setProjectTitle,
  projectDescription,
  setProjectDescription,
  projectHashtags,
  setProjectHashtags,
}) {
  const [projects, setProjects] = useState([]);
  const [page, setPage] = useState(0);
  const [loadError, setLoadError] = useState("");
  // Ids of every project with a background job actually running right now
  // -- from the store's own `jobs` state (see projectStore.js's own doc
  // comment on why this can't come from `list_projects`, which only ever
  // reflects disk state). Drives the "processing" status (this always
  // wins over every other derived status -- see projectStatus.js) and the
  // per-row spinner below.
  const processingIds = useAnyProjectProcessing();
  // Full-text keyword search over the library (library.rs's
  // `search_projects`, SQLite FTS5) -- `searchResults` is `null` while no
  // search is active (renders the normal paginated/sorted list); an empty
  // array is a real "no matches", distinct from "haven't searched yet".
  // Debounced client-side rather than firing on every keystroke, purely
  // to avoid a query per half-typed word -- FTS5 itself answers
  // synchronously, in-process, with no warm-up cost the way the earlier
  // embedding-based search needed.
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  // Date-range filter, combinable with the keyword search above (applied
  // client-side on top of whichever list -- search results or the full
  // library -- is currently in play).
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [newProjectModalOpen, setNewProjectModalOpen] = useState(false);
  // The whole project row currently open in EditProjectModal.jsx, or
  // `null` -- not just a boolean, since any row can be edited now, not
  // only the currently-open project. `onSave` below branches on whether
  // this happens to be the live/currently-open project.
  const [editingProject, setEditingProject] = useState(null);
  // The whole project row currently open in NewProjectModal.jsx's edit
  // mode (reopening a saved AI content-strategy draft), or `null`.
  const [editingDraft, setEditingDraft] = useState(null);
  // The whole project row whose original video is open in
  // VideoPreviewModal.jsx, or `null`.
  const [previewingProject, setPreviewingProject] = useState(null);

  async function refresh() {
    try {
      const list = await invoke("list_projects");
      setProjects(list);
      setLoadError("");
    } catch (err) {
      setLoadError(String(err));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

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
  // to the top of its own list on every autosave tick, and deliberately
  // never re-sorted just because a row was *selected* either (requested
  // directly: opening/editing a project must never move it in this
  // list). list_projects already returns rows in this order, but
  // re-sorting here is cheap insurance if that ever changes.
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
  // A keyword search already comes back ranked by relevance (best match
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

  function handleRowClick(p) {
    if (isStrategyDraft(p)) {
      setEditingDraft(p);
      return;
    }
    onSelectProject(p);
  }

  function handleEditClick(e, p) {
    e.stopPropagation();
    if (isStrategyDraft(p)) {
      setEditingDraft(p);
    } else {
      setEditingProject(p);
    }
  }

  return (
    <aside className="shell-sidebar">
      <div className="shell-sidebar-heading project-list-heading">
        <span>
          Projects{sorted.length > 0 && <span className="project-list-count"> ({sorted.length})</span>}
        </span>
        <button type="button" className="icon-button new-project-button" title="New: upload a video or start an AI content strategy" onClick={() => setNewProjectModalOpen(true)}>
          +
        </button>
      </div>
      <div className="shell-sidebar-body">
        <input
          type="search"
          className="project-search-input"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search title, description, hashtags…"
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
          {pageItems.map((p) => {
            const draft = isStrategyDraft(p);
            return (
              <div
                key={p.id}
                className={p.id === currentProjectId ? "project-list-item active" : "project-list-item"}
                onClick={() => handleRowClick(p)}
              >
                <div className="project-list-title">
                  {processingIds.includes(p.id) && (
                    <span className="project-processing-badge" title="Processing in the background…" />
                  )}
                  <span className="project-list-title-text">{p.title || p.original_filename || "Untitled"}</span>
                  <StatusBadge statusKey={deriveProjectStatus(p, processingIds.includes(p.id))} />
                  <button type="button" className="icon-button" title="Edit" onClick={(e) => handleEditClick(e, p)}>
                    ✏️
                  </button>
                  {!draft && (
                    <button
                      type="button"
                      className="icon-button"
                      title="View original video"
                      onClick={(e) => {
                        e.stopPropagation();
                        setPreviewingProject(p);
                      }}
                    >
                      🎬
                    </button>
                  )}
                </div>
                {!draft && <div className="project-list-filename">{p.original_filename}</div>}
                <div className="project-list-dates">
                  <span>Uploaded {formatDateTime(p.created_at)}</span>
                </div>
              </div>
            );
          })}
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

      {newProjectModalOpen && (
        <NewProjectModal
          onPickVideo={handlePickVideo}
          onSaved={refresh}
          onClose={() => setNewProjectModalOpen(false)}
        />
      )}

      {editingDraft && (
        <NewProjectModal
          editingDraft={editingDraft}
          onSaved={refresh}
          onClose={() => setEditingDraft(null)}
        />
      )}

      {editingProject && (
        <EditProjectModal
          title={editingProject.id === currentProjectId ? projectTitle : editingProject.title}
          description={editingProject.id === currentProjectId ? projectDescription : editingProject.description}
          hashtags={editingProject.id === currentProjectId ? projectHashtags : editingProject.hashtags}
          onSave={async ({ title, description, hashtags }) => {
            if (editingProject.id === currentProjectId) {
              // The currently-open project's own live setters -- these
              // already autosave through App.jsx's per-project store.
              setProjectTitle(title);
              setProjectDescription(description);
              setProjectHashtags(hashtags);
            } else {
              // A different row: save straight through, bypassing the
              // live editing session entirely (it isn't loaded into it).
              await invoke("save_project", { project: { ...editingProject, title, description, hashtags } });
              await refresh();
            }
          }}
          onClose={() => setEditingProject(null)}
        />
      )}

      {previewingProject && (
        <VideoPreviewModal
          title={previewingProject.title || previewingProject.original_filename}
          originalPath={previewingProject.original_path}
          onClose={() => setPreviewingProject(null)}
        />
      )}
    </aside>
  );
}

export default Sidebar;
