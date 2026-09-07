// Persistent media library: every imported (original) video is copied
// into app-managed storage under app_data_dir()/media/<project_id>/ --
// never left at whatever OS path the file dialog returned -- and every
// burned/exported output is written into that same project's own
// media/<project_id>/processed/ subfolder, never an arbitrary
// save-dialog location. Keeping "original + every processed export"
// inside one self-contained per-project folder is deliberate: a future
// cloud-overflow phase (see the project's own plan doc) can move or
// restore one project's whole folder as a single unit.
//
// Metadata (this file's whole reason to exist) lives in a local SQLite
// database (`library.db`, `rusqlite` with the `bundled` feature -- SQLite's
// own C source compiled directly into this binary, verified with a real
// `cargo build` against this project's MinGW/GNU toolchain before writing
// any code around it, the same discipline every other native-library
// dependency in this project has needed since sherpa-onnx's MSVC-only
// prebuilt library became a real, confirmed build blocker).
//
// Search was originally semantic (cosine-similarity over
// `sentence-transformers` embeddings, via the `sqlite-vec` extension and a
// Python embedding server this app had to keep warm). Replaced with
// SQLite's own built-in FTS5 full-text index, requested directly (real,
// fast keyword/prefix search over title/description/hashtags/transcript,
// no vector search) -- verified directly, not assumed, via a standalone
// probe before writing any code around it (same discipline the original
// sqlite-vec integration used): confirmed FTS5 is compiled into this
// exact `rusqlite = { version = "0.32", features = ["bundled"] }` with no
// extra feature flag needed, confirmed `bm25()` ranking and prefix
// queries (`"term"*`) work, and confirmed a query-sanitization scheme
// (wrap every whitespace-split term in escaped double quotes plus a
// trailing `*`) survives FTS5's special query-syntax characters
// (apostrophes, hyphens, a lone `"`) without erroring. This is a strict
// improvement over the embedding-based version, not just a simplification:
// no Python process to keep warm, no ~19s model-load latency on first
// search, no separate re-embed step to keep in sync after every edit --
// the FTS index updates synchronously, in the same SQL transaction, as
// part of the exact same `insert_project`/`update_project_row` calls that
// already run on every save.
//
// The `projects` table stays deliberately thin: only what the project
// *list* itself needs to render/sort/search (id, paths, title/description/
// hashtags, timestamps) gets real columns. Everything else App.jsx's
// editing session needs to fully restore (transcript words, caption style,
// prosody, speakers, voiceover/music state, detected language, generated
// content ideas) is kept as one opaque `state` JSON text column -- Rust
// never needs to understand that shape, only the frontend does. This is
// deliberate, not laziness: `CaptionStyle` (captions.rs) is
// `Deserialize`-only, no `Serialize` -- round-tripping it back out to the
// frontend through a typed Rust struct isn't free, and an opaque blob
// sidesteps needing that (and three other structs) to grow a second,
// Rust-side mirror that has to stay in lockstep with the JS shape forever.
// (Same reasoning is why the frontend, not this module, derives a
// project's display *status* -- pending/transcribed/burned/etc. -- from
// that same opaque `state`; see projectStatus.js.)
//
// A connection is opened fresh per command rather than held in shared
// Tauri-managed state -- these are small, infrequent operations (list a
// few dozen rows, upsert one row on an autosave tick), and SQLite's own
// file-level locking (with a busy_timeout set below) handles the rare
// case of two commands landing at once without needing a Rust-side mutex
// on top of it. This mirrors scheduler.rs's own "load whole file, mutate,
// save whole file" simplicity, just backed by SQLite instead of JSON.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::util::{cli_path, emit_progress};

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create app data directory: {e}"))?;
    Ok(dir)
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("library.db"))
}

/// media/<id>/ -- created on demand. One level deeper than the flat
/// per-purpose files scheduler.rs/instagram.rs write directly into
/// app_data_dir(), since each project owns multiple files (the original
/// plus every processed export), not just one.
fn project_media_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("media").join(id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create project media directory: {e}"))?;
    Ok(dir)
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            original_path TEXT NOT NULL,
            original_filename TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            hashtags TEXT NOT NULL,
            last_burned_path TEXT,
            storage_location TEXT NOT NULL DEFAULT 'local',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            exported_at INTEGER,
            published_at INTEGER,
            state TEXT NOT NULL
        )",
        (),
    )
    .map_err(|e| format!("Couldn't initialize library database: {e}"))?;

    // `exported_at`/`published_at` were added after this table already
    // shipped -- `CREATE TABLE IF NOT EXISTS` above only applies to a
    // brand-new database, so an existing one needs these columns added
    // explicitly. SQLite has no `ADD COLUMN IF NOT EXISTS`; the "duplicate
    // column name" error this throws on every run *after* the first is
    // the documented way to make this idempotent, so it's deliberately
    // swallowed rather than propagated.
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN exported_at INTEGER", ());
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN published_at INTEGER", ());

    // Full-text index over exactly what a keyword search should match --
    // kept as a separate FTS5 table (not a `content=` table shadowing
    // `projects` directly) since the indexed text (a transcript excerpt)
    // isn't a real column on `projects` itself, and because keeping it
    // fully external, rebuilt explicitly by `sync_fts_index` after every
    // insert/update, means a `projects` schema change never risks silently
    // breaking the FTS table's own column mapping. `project_id UNINDEXED`
    // is stored but never matched against -- it's how a ranked hit is
    // mapped back to a real `projects` row.
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS projects_fts USING fts5(
            project_id UNINDEXED, title, description, hashtags, transcript
        )",
        (),
    )
    .map_err(|e| format!("Couldn't initialize full-text search table: {e}"))?;

    // Small generic per-user key/value store -- deliberately not one table
    // per setting; today's only occupant is `default_caption_style` (the
    // saved-tweaked-style-becomes-the-new-default feature), but this stays
    // the natural place for any future per-user app preference without a
    // schema migration each time. Keyed by Firebase Auth's `user.uid`
    // (this app's only identity scheme, already wired in AuthContext.jsx)
    // rather than a single global row -- a tweak made by one signed-in user
    // shouldn't silently become another user's default on a shared machine.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS user_settings (
            user_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (user_id, key)
        )",
        (),
    )
    .map_err(|e| format!("Couldn't initialize user settings table: {e}"))?;

    Ok(())
}

fn open_db(app: &AppHandle) -> Result<Connection, String> {
    let conn = Connection::open(db_path(app)?).map_err(|e| format!("Couldn't open library database: {e}"))?;
    // Real, if rare, concurrent access is possible: an autosave tick and a
    // list refresh landing at nearly the same instant. A short busy
    // timeout (SQLite retries internally rather than failing immediately)
    // is cheap insurance against a spurious "database is locked" error
    // instead of a Rust-side mutex on top of SQLite's own file locking.
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("Couldn't configure database: {e}"))?;
    init_schema(&conn)?;
    Ok(conn)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    /// App-managed copy's path (media/<id>/original.<ext>) -- NOT the
    /// original OS path the user picked. Every downstream operation
    /// (transcription, burning, util.rs's transcript-freshness
    /// fingerprint) runs against this path from the moment of import
    /// onward, so the original file can be moved, renamed, or deleted
    /// afterward without breaking the project.
    pub original_path: String,
    /// The original OS filename (e.g. "IMG_1234.MOV"), kept purely for
    /// display -- `original_path`'s own filename is always
    /// "original.<ext>" and isn't meaningful to show a user.
    pub original_filename: String,
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
    pub last_burned_path: Option<String>,
    #[serde(default = "default_storage_location")]
    pub storage_location: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Set once, the first time `export_file` successfully copies this
    /// project's burned output out to a user-picked destination -- distinct
    /// from `last_burned_path` (which only means "a burn happened," not
    /// "the user ever took it out of the app"). Never cleared automatically
    /// by a later re-burn; a re-export just re-stamps it. Purely a display
    /// signal for `projectStatus.js` -- nothing else in this app reads it.
    #[serde(default)]
    pub exported_at: Option<i64>,
    /// Set once, the first time a post using this project's video
    /// successfully goes out via `post_to_instagram_now`/scheduling
    /// (instagram.rs) -- same "stamp once, never auto-clear" shape as
    /// `exported_at` above, and the same purely-a-display-signal role.
    #[serde(default)]
    pub published_at: Option<i64>,
    /// Opaque to Rust -- see this module's own doc comment above. The
    /// frontend assembles/reads this; Rust only ever stores and returns it
    /// verbatim.
    #[serde(default)]
    pub state: serde_json::Value,
}

fn default_storage_location() -> String {
    "local".to_string()
}

fn project_from_row(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    let hashtags_json: String = row.get("hashtags")?;
    let state_json: String = row.get("state")?;
    Ok(Project {
        id: row.get("id")?,
        original_path: row.get("original_path")?,
        original_filename: row.get("original_filename")?,
        title: row.get("title")?,
        description: row.get("description")?,
        hashtags: serde_json::from_str(&hashtags_json).unwrap_or_default(),
        last_burned_path: row.get("last_burned_path")?,
        storage_location: row.get("storage_location")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        exported_at: row.get("exported_at")?,
        published_at: row.get("published_at")?,
        state: serde_json::from_str(&state_json).unwrap_or(serde_json::json!({})),
    })
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Same nanosecond-timestamp-as-hex scheme as scheduler.rs's own
/// generate_id() -- duplicated rather than shared, matching this crate's
/// existing per-module self-containment (every ID-needing module already
/// has its own copy of whatever scheme it needs rather than importing one
/// from a sibling).
fn generate_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}")
}

fn extension_of(path: &str) -> String {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string()
}

fn filename_of(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

/// Placeholder title from a bare filename stem, until the user (or a
/// "Generate title, description & hashtags" run) sets a real one -- e.g.
/// "my_reel_final" -> "my reel final".
fn placeholder_title(filename: &str) -> String {
    let stem = std::path::Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    stem.replace(['_', '-'], " ")
}

/// Chunked copy with progress -- same shape as model_fetch.rs's chunked
/// download loop (whole-percent-step throttling, one `emit_progress` call
/// per step, not per chunk). A plain blocking `tokio::fs::copy` would give
/// no feedback at all on a large clip from a slow (HDD/USB/network) drive,
/// and reels source clips are frequently hundreds of MB to a couple GB.
async fn copy_with_progress(app: &AppHandle, src: &str, dest: &std::path::Path, project_id: &str) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let total = tokio::fs::metadata(src).await.map(|m| m.len()).unwrap_or(0);
    let mut reader = tokio::fs::File::open(src).await.map_err(|e| format!("Couldn't open the video file: {e}"))?;
    let mut writer = tokio::fs::File::create(dest).await.map_err(|e| format!("Couldn't create the library file: {e}"))?;
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut copied: u64 = 0;
    let mut last_reported_percent = -1.0;
    loop {
        let n = reader.read(&mut buf).await.map_err(|e| format!("Couldn't read the video file: {e}"))?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await.map_err(|e| format!("Couldn't write the library file: {e}"))?;
        copied += n as u64;
        if total > 0 {
            let percent = (copied as f64 / total as f64 * 100.0).clamp(0.0, 100.0);
            if percent - last_reported_percent >= 1.0 {
                last_reported_percent = percent;
                emit_progress(app, "import-progress", project_id, "copying", Some(percent), None);
            }
        }
    }
    Ok(())
}

/// Copies `video_path` into new app-managed storage, creates a new
/// `Project` row, persists it, and returns it. This is the only way a
/// video enters the library -- from this point on, the frontend (and
/// transcription, and util.rs's transcript-freshness fingerprint) works
/// against `original_path` (the copy), never the OS path the user picked.
#[tauri::command]
pub async fn create_project(app: AppHandle, video_path: String) -> Result<Project, String> {
    let id = generate_id();
    let media_dir = project_media_dir(&app, &id)?;
    let ext = extension_of(&video_path);
    let dest = media_dir.join(format!("original.{ext}"));

    emit_progress(&app, "import-progress", &id, "copying", Some(0.0), None);
    copy_with_progress(&app, &video_path, &dest, &id).await?;
    emit_progress(&app, "import-progress", &id, "done", Some(100.0), None);

    let filename = filename_of(&video_path);
    let now = now_unix_seconds();
    let project = Project {
        id,
        original_path: cli_path(&dest),
        original_filename: filename.clone(),
        title: placeholder_title(&filename),
        description: String::new(),
        hashtags: Vec::new(),
        last_burned_path: None,
        storage_location: default_storage_location(),
        created_at: now,
        updated_at: now,
        exported_at: None,
        published_at: None,
        state: serde_json::json!({}),
    };

    let conn = open_db(&app)?;
    insert_project(&conn, &project)?;
    Ok(project)
}

/// Creates *or updates* a project row for an AI content-strategy
/// *draft* -- no video exists yet, only a plan for one (a future
/// cloud-API generation feature this only collects the parameters for;
/// see `state.contentStrategyDraft` below). One command handles both
/// (`id: None` creates, `id: Some(existing)` updates in place) rather
/// than a separate update path, specifically so both go through the same
/// character-photo handling below -- a naive direct `save_project` call
/// for the edit case would let a *changed* reference photo's raw OS path
/// leak into `state` unconverted, since only this command's own copy step
/// ever turns a raw picker path into a real, app-managed one.
///
/// Deliberately a separate command from `create_project` rather than
/// teaching it to accept an optional video: `create_project`'s own
/// already-tested "copy a real file, stamp its path" behavior stays
/// completely untouched, and this only shares its id/media-dir setup, not
/// its video-copying logic.
///
/// `original_path`/`original_filename` are stored as plain empty strings
/// -- both columns are `NOT NULL`, but nothing else in this schema
/// requires them non-empty, and the frontend already has a real, existing
/// "no video" fallback for a falsy `original_path` (`MainPanel.jsx`'s own
/// empty state) -- `projectStatus.js` is what actually turns this into a
/// real "Drafted" status rather than a broken-looking blank project.
///
/// `character_photo_path`, when given, is a *fresh* OS path from the
/// picker -- copied into this project's own media folder the same "never
/// trust the OS picker's own path past import" way `create_project`
/// copies a video, and it's that app-managed copy's path that's stored,
/// never the original. When omitted on an update (the user didn't pick a
/// new one), the previously-stored `characterPhotoPath` is carried
/// forward unchanged rather than being cleared.
#[tauri::command]
pub async fn create_strategy_draft(
    app: AppHandle,
    id: Option<String>,
    title: String,
    description: String,
    params: serde_json::Value,
    character_photo_path: Option<String>,
) -> Result<Project, String> {
    let conn = open_db(&app)?;

    let (project_id, media_dir, created_at, previous_photo_path, is_new) = match &id {
        Some(existing_id) => {
            let existing = query_project_by_id(&conn, existing_id)?;
            let previous_photo_path = existing
                .state
                .get("contentStrategyDraft")
                .and_then(|d| d.get("characterPhotoPath"))
                .and_then(|p| p.as_str())
                .map(String::from);
            (existing_id.clone(), project_media_dir(&app, existing_id)?, existing.created_at, previous_photo_path, false)
        }
        None => {
            let new_id = generate_id();
            let media_dir = project_media_dir(&app, &new_id)?;
            (new_id, media_dir, now_unix_seconds(), None, true)
        }
    };

    let stored_photo_path = match character_photo_path {
        Some(src) => {
            let ext = extension_of(&src);
            let dest = media_dir.join(format!("character-reference.{ext}"));
            tokio::fs::copy(&src, &dest).await.map_err(|e| format!("Couldn't save the character reference photo: {e}"))?;
            Some(cli_path(&dest))
        }
        None => previous_photo_path,
    };

    let mut state = params;
    if let serde_json::Value::Object(map) = &mut state {
        map.insert("characterPhotoPath".to_string(), stored_photo_path.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null));
    }

    let project = Project {
        id: project_id,
        original_path: String::new(),
        original_filename: String::new(),
        title,
        description,
        hashtags: Vec::new(),
        last_burned_path: None,
        storage_location: default_storage_location(),
        created_at,
        updated_at: now_unix_seconds(),
        exported_at: None,
        published_at: None,
        state: serde_json::json!({ "contentStrategyDraft": state }),
    };

    if is_new {
        insert_project(&conn, &project)?;
        Ok(project)
    } else {
        update_project_row(&conn, project)
    }
}

/// Pure SQL, no `AppHandle` -- separated from `create_project` so it's
/// directly testable against an in-memory connection (see the `tests`
/// module below), not just exercisable through the full Tauri command
/// pipeline.
fn insert_project(conn: &Connection, project: &Project) -> Result<(), String> {
    conn.execute(
        "INSERT INTO projects (id, original_path, original_filename, title, description, hashtags, last_burned_path, storage_location, created_at, updated_at, exported_at, published_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        rusqlite::params![
            project.id,
            project.original_path,
            project.original_filename,
            project.title,
            project.description,
            serde_json::to_string(&project.hashtags).unwrap_or_default(),
            project.last_burned_path,
            project.storage_location,
            project.created_at,
            project.updated_at,
            project.exported_at,
            project.published_at,
            serde_json::to_string(&project.state).unwrap_or_default(),
        ],
    )
    .map_err(|e| format!("Couldn't save the new project: {e}"))?;
    sync_fts_index(conn, project)?;
    Ok(())
}

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let conn = open_db(&app)?;
    query_all_projects(&conn)
}

/// Fetches one project fresh from disk by id -- used when *opening* a
/// project (Sidebar.jsx's click handler), deliberately never the copy
/// sitting in Sidebar's own `list_projects`-fetched array. That array is
/// only ever refreshed on mount and right after a new import (see
/// Sidebar.jsx's own doc comment) -- NOT after every autosave tick, a burn,
/// or a content-strategy generation -- so by the time a user clicks a
/// project (including the one they're already editing), that cached row
/// can be many minutes stale. Applying a stale row over a live editing
/// session silently discarded real, already-persisted-elsewhere progress
/// (confirmed directly: a real project's `words`/`contentIdeas`/
/// `last_burned_path` all reverted to empty after being clicked again in
/// the sidebar) -- this command exists specifically so App.jsx's
/// `loadProject` always applies what's truly in the database right now,
/// never a snapshot from whenever the sidebar last happened to fetch.
#[tauri::command]
pub fn get_project(app: AppHandle, id: String) -> Result<Project, String> {
    let conn = open_db(&app)?;
    query_project_by_id(&conn, &id)
}

fn query_project_by_id(conn: &Connection, id: &str) -> Result<Project, String> {
    conn.query_row("SELECT * FROM projects WHERE id = ?1", rusqlite::params![id], project_from_row)
        .map_err(|e| format!("Couldn't load project {id}: {e}"))
}

fn query_all_projects(conn: &Connection) -> Result<Vec<Project>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM projects ORDER BY created_at DESC")
        .map_err(|e| format!("Couldn't query projects: {e}"))?;
    let rows = stmt
        .query_map([], project_from_row)
        .map_err(|e| format!("Couldn't query projects: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("Couldn't read projects: {e}"))
}

/// Whole-row upsert by id -- autosave, title/description/hashtag edits,
/// and the post-burn `last_burned_path` update all go through this one
/// command, same "load, mutate the one entry, save" shape as
/// scheduler.rs's own mutation commands. Always re-stamps `updated_at`
/// server-side so the frontend can't get that wrong, and never touches
/// `created_at` -- the list's sort order has to stay stable across edits.
#[tauri::command]
pub fn save_project(app: AppHandle, project: Project) -> Result<Project, String> {
    let conn = open_db(&app)?;
    update_project_row(&conn, project)
}

fn update_project_row(conn: &Connection, project: Project) -> Result<Project, String> {
    let updated_at = now_unix_seconds();
    let rows_changed = conn
        .execute(
            "UPDATE projects SET original_path = ?1, original_filename = ?2, title = ?3, description = ?4,
             hashtags = ?5, last_burned_path = ?6, storage_location = ?7, updated_at = ?8, exported_at = ?9, published_at = ?10, state = ?11
             WHERE id = ?12",
            rusqlite::params![
                project.original_path,
                project.original_filename,
                project.title,
                project.description,
                serde_json::to_string(&project.hashtags).unwrap_or_default(),
                project.last_burned_path,
                project.storage_location,
                updated_at,
                project.exported_at,
                project.published_at,
                serde_json::to_string(&project.state).unwrap_or_default(),
                project.id,
            ],
        )
        .map_err(|e| format!("Couldn't save project: {e}"))?;

    if rows_changed == 0 {
        return Err(format!("Project {} not found", project.id));
    }
    let saved = Project { updated_at, ..project };
    sync_fts_index(conn, &saved)?;
    Ok(saved)
}

#[tauri::command]
pub async fn delete_project(app: AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM projects WHERE id = ?1", [&id]).map_err(|e| format!("Couldn't delete project: {e}"))?;
    let _ = conn.execute("DELETE FROM projects_fts WHERE project_id = ?1", rusqlite::params![id]);
    // Best-effort -- an already-missing folder (e.g. manually deleted by
    // the user) shouldn't block removing the database row.
    if let Ok(dir) = project_media_dir(&app, &id) {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    Ok(())
}

/// Text a project's FTS row is built from -- title + description +
/// hashtags + a transcript excerpt if one exists. Mirrors the old
/// embedding-based version's exact same scope, for the same reason:
/// `description` already carries the topic in a couple of sentences
/// (content_ideas.rs), so indexing the *whole* transcript would mostly add
/// filler noise -- the first ~120 words (roughly a video's opening/hook)
/// is plenty of extra searchable text for a project that's only been
/// transcribed, not yet run through content strategy.
fn transcript_excerpt(project: &Project) -> String {
    project
        .state
        .get("words")
        .and_then(|w| w.as_array())
        .map(|arr| arr.iter().filter_map(|w| w.get("word").and_then(|w| w.as_str())).take(120).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Rebuilds one project's FTS row -- delete-then-insert (FTS5 has no
/// native upsert), called synchronously right after every insert/update so
/// the index is never more than the same transaction behind `projects`
/// itself. No separate debounced re-embed step, no Python round trip --
/// unlike the embedding version this replaced, indexing here is just more
/// SQL against the same already-open connection.
fn sync_fts_index(conn: &Connection, project: &Project) -> Result<(), String> {
    conn.execute("DELETE FROM projects_fts WHERE project_id = ?1", rusqlite::params![project.id])
        .map_err(|e| format!("Couldn't clear old search index entry: {e}"))?;
    conn.execute(
        "INSERT INTO projects_fts (project_id, title, description, hashtags, transcript) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![project.id, project.title, project.description, project.hashtags.join(" "), transcript_excerpt(project)],
    )
    .map_err(|e| format!("Couldn't update search index: {e}"))?;
    Ok(())
}

/// Turns raw user input into a safe, useful FTS5 MATCH expression: each
/// whitespace-split term becomes its own escaped, double-quoted, trailing-
/// `*` phrase (`foo bar` -> `"foo"* "bar"*`), ANDed together (FTS5's
/// default for multiple bare terms). Quoting neutralizes every character
/// FTS5's own query syntax treats specially (`"`, `*`, `:`, `-`, a lone
/// unmatched quote) -- verified directly, not assumed, against a real
/// FTS5 table before shipping: "don't", "co-founder", and a bare `"`
/// character all round-tripped without a syntax error, and a real prefix
/// query ("gar" matching both "garlic" and "Garage") came back correctly
/// ranked. An input that's empty after trimming, or entirely made of
/// whitespace, produces an empty string here -- callers must check that
/// themselves before running it as a query (an empty MATCH expression is
/// a real SQL error, not "match everything").
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Full-text search over the library (title, description, hashtags, and a
/// transcript excerpt), ranked by SQLite's own built-in `bm25()` relevance
/// score. Replaced the original semantic/embedding-based version --
/// requested directly, for two real, concrete wins: no Python embedding
/// process to keep warm (an FTS5 MATCH query is answered by SQLite itself,
/// synchronously, in the same process), and no ~19s model-load latency the
/// first time a user searches in a fresh app session. A project with no
/// indexed text yet (brand new, untranscribed, everything still blank)
/// simply can't match anything -- there's nothing stale about that, just
/// nothing indexed yet, exactly like the version this replaced.
#[tauri::command]
pub fn search_projects(app: AppHandle, query: String) -> Result<Vec<Project>, String> {
    let sanitized = sanitize_fts_query(query.trim());
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    let conn = open_db(&app)?;

    let mut stmt = conn
        .prepare("SELECT project_id FROM projects_fts WHERE projects_fts MATCH ?1 ORDER BY bm25(projects_fts)")
        .map_err(|e| format!("Couldn't prepare search query: {e}"))?;
    let ranked_ids: Vec<String> = stmt
        .query_map(rusqlite::params![sanitized], |r| r.get(0))
        .map_err(|e| format!("Couldn't run search query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Couldn't read search results: {e}"))?;

    // One lookup per ranked id rather than a second bulk query -- this
    // app's realistic scale (client-side pagination at ~8/page in
    // Sidebar.jsx) never justifies more, same reasoning that keeps
    // `list_projects` a plain `SELECT *` with no server-side paging.
    ranked_ids.iter().map(|id| query_project_by_id(&conn, id)).collect()
}

/// Stamps `exported_at` the moment `export_file` (below) successfully
/// copies a project's burned output out to a user-picked destination --
/// see `Project::exported_at`'s own doc comment for why this is tracked
/// separately from `last_burned_path`. Called by the frontend right after
/// a successful `export_file`, not folded into that command itself, since
/// `export_file` takes bare paths (no project id) and is reused as-is.
#[tauri::command]
pub fn mark_project_exported(app: AppHandle, id: String) -> Result<Project, String> {
    let conn = open_db(&app)?;
    let mut project = query_project_by_id(&conn, &id)?;
    project.exported_at = Some(now_unix_seconds());
    update_project_row(&conn, project)
}

/// Same shape as `mark_project_exported` above, stamping `published_at`
/// instead -- called by the frontend right after a successful
/// `post_to_instagram_now` (instagram.rs), which itself has no notion of
/// "which project" (it takes a bare video path).
#[tauri::command]
pub fn mark_project_published(app: AppHandle, id: String) -> Result<Project, String> {
    let conn = open_db(&app)?;
    let mut project = query_project_by_id(&conn, &id)?;
    project.published_at = Some(now_unix_seconds());
    update_project_row(&conn, project)
}

/// Computes (and creates the containing directory for) a fresh app-managed
/// path for a burned/exported output, inside this project's own
/// media/<id>/processed/ folder. The frontend calls this in place of the
/// save-dialog it used to show, then passes the result straight through,
/// unchanged, to the existing `burn_captions` command as `output_path` --
/// `burn_captions` itself needed zero changes for this.
#[tauri::command]
pub fn processed_output_path(app: AppHandle, project_id: String) -> Result<String, String> {
    let processed_dir = project_media_dir(&app, &project_id)?.join("processed");
    std::fs::create_dir_all(&processed_dir).map_err(|e| format!("Couldn't create the processed output directory: {e}"))?;
    Ok(cli_path(&processed_dir.join(format!("burned-{}.mp4", generate_id()))))
}

/// Opens the OS file manager with `path` pre-selected -- lets a user jump
/// from "Burn & Export finished" straight to the app-managed processed
/// folder without needing to know where app_data_dir() actually lives.
/// Thin wrapper over the plugin's own function, same pattern as
/// instagram.rs's open_external_url wrapping tauri_plugin_opener::open_url.
#[tauri::command]
pub fn reveal_in_folder(path: String) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| format!("Couldn't reveal the file: {e}"))
}

const DEFAULT_CAPTION_STYLE_KEY: &str = "default_caption_style";

/// What actually gets saved: the base theme's id (`src/lib/themes.js`'s
/// `CAPTION_THEMES`) the user started from, plus the (possibly hand-tweaked)
/// style itself. Deliberately no separate "display name" or "is this
/// customized" flag stored here -- the frontend derives both, every time,
/// by comparing `style` against `theme_id`'s own stock style (exact match =
/// the theme's own name; anything else = "<theme name> (Custom)"), so
/// there's exactly one source of truth for that comparison rather than a
/// stored label that could silently drift out of sync with `style` itself.
/// `style` stays `serde_json::Value`, not the strict `CaptionStyle` struct,
/// for the same reason `Project::state` does (see this module's own doc
/// comment) -- `CaptionStyle` is `Deserialize`-only, and Rust never needs to
/// understand this shape, only round-trip it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultCaptionStyle {
    pub theme_id: String,
    pub style: serde_json::Value,
}

/// Fetches the signed-in user's saved default caption style, if they've
/// ever tweaked one -- `None` means "never customized," and the frontend
/// falls back to its own hardcoded factory default (Cascade Bold) in that
/// case, exactly as if this command didn't exist yet.
#[tauri::command]
pub fn get_default_caption_style(app: AppHandle, user_id: String) -> Result<Option<DefaultCaptionStyle>, String> {
    let conn = open_db(&app)?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM user_settings WHERE user_id = ?1 AND key = ?2",
            rusqlite::params![user_id, DEFAULT_CAPTION_STYLE_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Couldn't read the saved default caption style: {e}"))?;
    match raw {
        Some(json) => {
            serde_json::from_str(&json).map(Some).map_err(|e| format!("Saved default caption style is corrupt: {e}"))
        }
        None => Ok(None),
    }
}

/// Upserts the signed-in user's default caption style -- called (debounced)
/// from the frontend every time the caption style actually changes while a
/// project is open, whether by picking a different theme card or by hand-
/// tweaking a field. The *next* new project this user creates starts from
/// whatever this last saved, instead of always the hardcoded factory
/// default -- see App.jsx's `projectSliceFromProject`.
#[tauri::command]
pub fn save_default_caption_style(app: AppHandle, user_id: String, theme_id: String, style: serde_json::Value) -> Result<(), String> {
    let conn = open_db(&app)?;
    let payload = DefaultCaptionStyle { theme_id, style };
    let json = serde_json::to_string(&payload).map_err(|e| format!("Couldn't serialize default caption style: {e}"))?;
    conn.execute(
        "INSERT INTO user_settings (user_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![user_id, DEFAULT_CAPTION_STYLE_KEY, json],
    )
    .map_err(|e| format!("Couldn't save default caption style: {e}"))?;
    Ok(())
}

/// Clears the signed-in user's saved default -- the "reset to factory
/// default" escape hatch. Only removes the *saved default* row; the
/// frontend is responsible for also resetting whatever project is
/// currently open back to the factory style (this command alone doesn't
/// touch any project's own already-persisted `state.captionStyle`).
#[tauri::command]
pub fn reset_default_caption_style(app: AppHandle, user_id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM user_settings WHERE user_id = ?1 AND key = ?2", rusqlite::params![user_id, DEFAULT_CAPTION_STYLE_KEY])
        .map_err(|e| format!("Couldn't reset default caption style: {e}"))?;
    Ok(())
}

/// Copies a burned/exported video out of app-managed storage to wherever
/// the user picked in a save dialog -- the "Download" option in
/// ExportButton.jsx. `source` is always `last_burned_path`
/// (media/<id>/processed/burned-*.mp4), never touched or moved by this;
/// the app's own copy stays exactly where it is so re-opening the project
/// later still finds it.
#[tauri::command]
pub async fn export_file(source: String, destination: String) -> Result<(), String> {
    tokio::fs::copy(&source, &destination).await.map_err(|e| format!("Couldn't export the file: {e}"))?;
    Ok(())
}

// NOTE: `cargo test` for this whole crate currently crashes with
// STATUS_ENTRYPOINT_NOT_FOUND before running any test at all -- confirmed
// directly to be pre-existing and unrelated to this module (reproduced on
// a clean `git stash` of every change from this session, including before
// `library.rs`/`rusqlite` existed at all). These tests are still worth
// having (same convention as captions.rs's own suite) for whenever that
// gets fixed; in the meantime this exact logic was verified for real via
// a standalone `cargo run` binary outside this crate (same schema/
// insert/query/update SQL, a real rusqlite connection, real UTF-8 Tamil
// text through the JSON `state` column) -- all 11 checks passed.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project(id: &str, created_at: i64) -> Project {
        Project {
            id: id.to_string(),
            original_path: format!(r"C:\lib\media\{id}\original.mp4"),
            original_filename: "IMG_1234.MOV".to_string(),
            title: "a title".to_string(),
            description: String::new(),
            hashtags: vec!["#one".to_string(), "#two".to_string()],
            last_burned_path: None,
            storage_location: default_storage_location(),
            created_at,
            updated_at: created_at,
            exported_at: None,
            published_at: None,
            state: serde_json::json!({"words": [{"word": "பாத்துக்கலாம்", "start": 0.0, "end": 0.5}]}),
        }
    }

    #[test]
    fn list_orders_newest_first_and_round_trips_json_columns() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_project(&conn, &sample_project("aaa", 100)).unwrap();
        insert_project(&conn, &sample_project("bbb", 200)).unwrap();

        let listed = query_all_projects(&conn).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "bbb", "newest created_at should sort first");
        assert_eq!(listed[0].hashtags, vec!["#one".to_string(), "#two".to_string()]);
        assert_eq!(listed[0].state["words"][0]["word"].as_str().unwrap(), "பாத்துக்கலாம்");
    }

    #[test]
    fn update_changes_only_the_targeted_row_and_preserves_created_at() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_project(&conn, &sample_project("aaa", 100)).unwrap();

        let mut edit = sample_project("aaa", 100);
        edit.title = "edited title".to_string();
        let saved = update_project_row(&conn, edit).unwrap();
        assert_eq!(saved.title, "edited title");
        assert_eq!(saved.created_at, 100, "editing must never move a project in the newest-first list");
        assert_ne!(saved.updated_at, 100, "updated_at must be re-stamped, not passed through");
    }

    #[test]
    fn update_on_missing_id_is_a_real_error_not_a_silent_no_op() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let result = update_project_row(&conn, sample_project("does-not-exist", 100));
        assert!(result.is_err());
    }

    /// Guards the exact bug `get_project` was added to fix: a project must
    /// come back reflecting the most recent `update_project_row` write, not
    /// whatever it looked like at insert time -- App.jsx's `loadProject`
    /// depends on this to never apply a stale snapshot over live progress.
    #[test]
    fn get_by_id_reflects_the_latest_update_not_the_original_insert() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_project(&conn, &sample_project("aaa", 100)).unwrap();

        let mut edit = sample_project("aaa", 100);
        edit.state = serde_json::json!({"words": [{"word": "fresh", "start": 0.0, "end": 0.5}]});
        edit.last_burned_path = Some(r"C:\lib\media\aaa\processed\burned-aaa.mp4".to_string());
        update_project_row(&conn, edit).unwrap();

        let fetched = query_project_by_id(&conn, "aaa").unwrap();
        assert_eq!(fetched.state["words"][0]["word"].as_str().unwrap(), "fresh");
        assert_eq!(fetched.last_burned_path.as_deref(), Some(r"C:\lib\media\aaa\processed\burned-aaa.mp4"));
    }

    #[test]
    fn get_by_id_on_missing_id_is_a_real_error() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let result = query_project_by_id(&conn, "does-not-exist");
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_fts_query_wraps_each_term_and_neutralizes_special_characters() {
        assert_eq!(sanitize_fts_query("garage gym"), "\"garage\"* \"gym\"*");
        // A literal double quote inside a term is escaped by doubling it
        // (SQL string-literal convention), not stripped -- either way it
        // must never break out of the quoted phrase.
        assert_eq!(sanitize_fts_query("don't"), "\"don't\"*");
        assert_eq!(sanitize_fts_query("co-founder"), "\"co-founder\"*");
        assert_eq!(sanitize_fts_query("   "), "", "whitespace-only input must sanitize to empty, not a bare query");
    }

    #[test]
    fn search_finds_a_project_by_a_word_that_only_appears_in_its_transcript() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let mut cooking = sample_project("cooking", 100);
        cooking.title = "My Cooking Video".to_string();
        cooking.state = serde_json::json!({"words": [{"word": "garlic", "start": 0.0, "end": 0.5}, {"word": "pasta", "start": 0.5, "end": 1.0}]});
        insert_project(&conn, &cooking).unwrap();
        let mut gym = sample_project("gym", 200);
        gym.title = "Garage Gym Build".to_string();
        insert_project(&conn, &gym).unwrap();

        let hits: Vec<String> =
            search_projects_in(&conn, "garlic").unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(hits, vec!["cooking".to_string()]);
    }

    #[test]
    fn search_prefix_matches_across_multiple_fields_and_ranks_results() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let mut cooking = sample_project("cooking", 100);
        cooking.title = "My Cooking Video".to_string();
        insert_project(&conn, &cooking).unwrap();
        let mut gym = sample_project("gym", 200);
        gym.title = "Garage Gym Build".to_string();
        insert_project(&conn, &gym).unwrap();

        // "gar" is a real prefix of both "garlic" (only in cooking's own
        // sample_project transcript) and "Garage" (gym's title).
        let mut hits: Vec<String> = search_projects_in(&conn, "gar").unwrap().into_iter().map(|p| p.id).collect();
        hits.sort();
        assert_eq!(hits, vec!["cooking".to_string(), "gym".to_string()]);
    }

    #[test]
    fn search_reflects_an_edit_immediately_no_separate_reindex_step() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_project(&conn, &sample_project("aaa", 100)).unwrap();
        assert!(search_projects_in(&conn, "renovation").unwrap().is_empty());

        let mut edit = sample_project("aaa", 100);
        edit.title = "Kitchen Renovation".to_string();
        update_project_row(&conn, edit).unwrap();

        let hits: Vec<String> = search_projects_in(&conn, "renovation").unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(hits, vec!["aaa".to_string()]);
    }

    #[test]
    fn deleting_a_project_removes_it_from_search_too() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let mut project = sample_project("aaa", 100);
        project.title = "Unique Searchable Title".to_string();
        insert_project(&conn, &project).unwrap();
        assert_eq!(search_projects_in(&conn, "unique").unwrap().len(), 1);

        conn.execute("DELETE FROM projects WHERE id = ?1", ["aaa"]).unwrap();
        conn.execute("DELETE FROM projects_fts WHERE project_id = ?1", ["aaa"]).unwrap();

        assert!(search_projects_in(&conn, "unique").unwrap().is_empty());
    }

    /// `search_projects` itself takes an `AppHandle` (to open its own
    /// connection) -- this mirrors its actual query logic against an
    /// in-memory connection the same way `query_project_by_id`/
    /// `insert_project` already are tested, rather than duplicating the
    /// sanitize+prepare+query+lookup steps inline in every test above.
    fn search_projects_in(conn: &Connection, query: &str) -> Result<Vec<Project>, String> {
        let sanitized = sanitize_fts_query(query.trim());
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare("SELECT project_id FROM projects_fts WHERE projects_fts MATCH ?1 ORDER BY bm25(projects_fts)")
            .map_err(|e| e.to_string())?;
        let ranked_ids: Vec<String> =
            stmt.query_map(rusqlite::params![sanitized], |r| r.get(0)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        ranked_ids.iter().map(|id| query_project_by_id(conn, id)).collect()
    }

    #[test]
    fn mark_exported_stamps_exported_at_without_touching_created_at() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        insert_project(&conn, &sample_project("aaa", 100)).unwrap();
        assert_eq!(query_project_by_id(&conn, "aaa").unwrap().exported_at, None);

        let mut project = query_project_by_id(&conn, "aaa").unwrap();
        project.exported_at = Some(now_unix_seconds());
        update_project_row(&conn, project).unwrap();

        let fetched = query_project_by_id(&conn, "aaa").unwrap();
        assert!(fetched.exported_at.is_some());
        assert_eq!(fetched.created_at, 100, "marking exported must never move a project in the newest-first list");
    }
}
