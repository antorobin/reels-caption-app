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
// prebuilt library became a real, confirmed build blocker). SQLite (not
// the flat-JSON-file pattern scheduler.rs/instagram.rs use elsewhere) is
// the deliberate choice here specifically because a later phase adds local
// vector/cosine search over this same data (via the `sqlite-vec`
// extension) -- something a flat JSON file has no path to at all.
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
//
// A connection is opened fresh per command rather than held in shared
// Tauri-managed state -- these are small, infrequent operations (list a
// few dozen rows, upsert one row on an autosave tick), and SQLite's own
// file-level locking (with a busy_timeout set below) handles the rare
// case of two commands landing at once without needing a Rust-side mutex
// on top of it. This mirrors scheduler.rs's own "load whole file, mutate,
// save whole file" simplicity, just backed by SQLite instead of JSON.

use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Once;
use tauri::{AppHandle, Manager};
use zerocopy::AsBytes;

use crate::util::{cli_path, emit_progress};

/// all-MiniLM-L6-v2's native output size (embeddings.rs) -- fixed at the
/// vec0 virtual table's schema level, so this has to be a real constant,
/// not a runtime value.
const EMBEDDING_DIM: usize = 384;

static VEC_EXTENSION_REGISTERED: Once = Once::new();

/// Registers sqlite-vec's `vec0` virtual table type with SQLite's global
/// auto-extension mechanism -- process-wide, not per-connection, so this
/// only ever needs to run once no matter how many `open_db` calls follow
/// (this module opens a fresh connection per command, see this module's own
/// doc comment above). Verified directly via a standalone probe (a real
/// `vec0` table, real cosine-distance KNN query, correct ranking) before
/// wiring this in -- see Cargo.toml's own comment on the `sqlite-vec` entry.
fn ensure_vec_extension_registered() {
    VEC_EXTENSION_REGISTERED.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

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
            state TEXT NOT NULL
        )",
        (),
    )
    .map_err(|e| format!("Couldn't initialize library database: {e}"))?;

    // vec0 tables can only hold an integer rowid, but project ids are hex
    // strings (generate_id()) -- this mapping table is the bridge between
    // the two, an autoincrementing surrogate key rather than hashing the id
    // string (a hash risks collisions this app would have no way to detect;
    // an autoincrement key can't collide by construction).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS project_vec_ids (
            vec_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL UNIQUE
        )",
        (),
    )
    .map_err(|e| format!("Couldn't initialize embedding id table: {e}"))?;

    conn.execute(
        &format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS project_vecs USING vec0(embedding float[{EMBEDDING_DIM}] distance_metric=cosine)"
        ),
        (),
    )
    .map_err(|e| format!("Couldn't initialize embedding search table: {e}"))?;

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
    ensure_vec_extension_registered();
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
        state: serde_json::json!({}),
    };

    let conn = open_db(&app)?;
    insert_project(&conn, &project)?;
    Ok(project)
}

/// Pure SQL, no `AppHandle` -- separated from `create_project` so it's
/// directly testable against an in-memory connection (see the `tests`
/// module below), not just exercisable through the full Tauri command
/// pipeline.
fn insert_project(conn: &Connection, project: &Project) -> Result<(), String> {
    conn.execute(
        "INSERT INTO projects (id, original_path, original_filename, title, description, hashtags, last_burned_path, storage_location, created_at, updated_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            serde_json::to_string(&project.state).unwrap_or_default(),
        ],
    )
    .map_err(|e| format!("Couldn't save the new project: {e}"))?;
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
             hashtags = ?5, last_burned_path = ?6, storage_location = ?7, updated_at = ?8, state = ?9
             WHERE id = ?10",
            rusqlite::params![
                project.original_path,
                project.original_filename,
                project.title,
                project.description,
                serde_json::to_string(&project.hashtags).unwrap_or_default(),
                project.last_burned_path,
                project.storage_location,
                updated_at,
                serde_json::to_string(&project.state).unwrap_or_default(),
                project.id,
            ],
        )
        .map_err(|e| format!("Couldn't save project: {e}"))?;

    if rows_changed == 0 {
        return Err(format!("Project {} not found", project.id));
    }
    Ok(Project { updated_at, ..project })
}

#[tauri::command]
pub async fn delete_project(app: AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM projects WHERE id = ?1", [&id]).map_err(|e| format!("Couldn't delete project: {e}"))?;
    // Best-effort -- a project with no embedding yet (never searched/saved
    // since this feature shipped) has no matching rows here at all.
    if let Ok(Some(vec_rowid)) = lookup_vec_rowid(&conn, &id) {
        let _ = conn.execute("DELETE FROM project_vecs WHERE rowid = ?1", rusqlite::params![vec_rowid]);
        let _ = conn.execute("DELETE FROM project_vec_ids WHERE project_id = ?1", rusqlite::params![id]);
    }
    // Best-effort -- an already-missing folder (e.g. manually deleted by
    // the user) shouldn't block removing the database row.
    if let Ok(dir) = project_media_dir(&app, &id) {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    Ok(())
}

fn lookup_vec_rowid(conn: &Connection, project_id: &str) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT vec_rowid FROM project_vec_ids WHERE project_id = ?1",
        rusqlite::params![project_id],
        |r| r.get(0),
    )
    .map(Some)
    .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
}

/// Text a project's embedding is computed from: title + description +
/// hashtags + a transcript snippet if one exists. Description already
/// carries the topic in a couple of sentences (content_ideas.rs), so
/// embedding the *whole* transcript would mostly dilute that with filler --
/// the first ~120 words (roughly what a video's opening/hook covers) is
/// plenty of extra signal for a video with no generated description yet
/// (e.g. one that's only been transcribed, not run through content
/// strategy) without needing the same token-budget machinery llm_budget.rs
/// has to worry about (this only feeds a fixed-size embedding, not a
/// prompt with a hard context ceiling).
fn embeddable_text(project: &Project) -> String {
    let words: Vec<&str> = project
        .state
        .get("words")
        .and_then(|w| w.as_array())
        .map(|arr| arr.iter().filter_map(|w| w.get("word").and_then(|w| w.as_str())).take(120).collect())
        .unwrap_or_default();
    let transcript_snippet = words.join(" ");

    [project.title.as_str(), project.description.as_str(), &project.hashtags.join(" "), &transcript_snippet]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(". ")
}

/// (Re-)computes and stores one project's embedding, called by the
/// frontend at a few natural checkpoints (a fresh content-strategy result,
/// a coarse debounce on manual title/description edits) rather than on
/// every autosave tick -- embedding still has to go through the same
/// Python process embeddings.rs manages, and while that process stays warm
/// (unlike llm.rs's one-shot-per-call siblings), there's no reason to
/// re-embed on every single keystroke either.
#[tauri::command]
pub async fn reembed_project(app: AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    let project = query_project_by_id(&conn, &id)?;
    let text = embeddable_text(&project);
    if text.is_empty() {
        return Ok(()); // Nothing to embed yet (e.g. a brand-new, untranscribed project).
    }
    let embedding = crate::embeddings::embed(&app, &text).await?;
    upsert_project_embedding(&conn, &id, &embedding)
}

fn upsert_project_embedding(conn: &Connection, project_id: &str, embedding: &[f32]) -> Result<(), String> {
    conn.execute("INSERT OR IGNORE INTO project_vec_ids (project_id) VALUES (?1)", rusqlite::params![project_id])
        .map_err(|e| format!("Couldn't register project for search: {e}"))?;
    let vec_rowid = lookup_vec_rowid(conn, project_id)
        .map_err(|e| format!("Couldn't look up project's search id: {e}"))?
        .ok_or_else(|| "Couldn't register project for search".to_string())?;

    // vec0 has no ON CONFLICT/UPSERT support -- delete-then-insert is the
    // documented safe pattern for "replace this row's embedding".
    conn.execute("DELETE FROM project_vecs WHERE rowid = ?1", rusqlite::params![vec_rowid])
        .map_err(|e| format!("Couldn't clear old embedding: {e}"))?;
    conn.execute(
        "INSERT INTO project_vecs (rowid, embedding) VALUES (?1, ?2)",
        rusqlite::params![vec_rowid, embedding.as_bytes()],
    )
    .map_err(|e| format!("Couldn't store embedding: {e}"))?;
    Ok(())
}

/// Semantic search over the library: embeds `query`, runs a real
/// cosine-distance KNN lookup (verified directly -- see Cargo.toml's
/// comment on the `sqlite-vec` entry), and returns matching projects
/// ranked nearest-first. Projects with no embedding yet (never saved/
/// generated since this feature shipped, or `embeddable_text` came back
/// empty) simply can't match -- there's nothing stale about that, just
/// nothing indexed yet.
#[tauri::command]
pub async fn search_projects(app: AppHandle, query: String) -> Result<Vec<Project>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let embedding = crate::embeddings::embed(&app, query).await?;
    let conn = open_db(&app)?;

    let mut stmt = conn
        .prepare(
            "SELECT project_vec_ids.project_id, project_vecs.distance
             FROM project_vecs
             JOIN project_vec_ids ON project_vecs.rowid = project_vec_ids.vec_rowid
             WHERE project_vecs.embedding MATCH ?1 AND k = 50
             ORDER BY project_vecs.distance",
        )
        .map_err(|e| format!("Couldn't prepare search query: {e}"))?;
    let ranked_ids: Vec<String> = stmt
        .query_map(rusqlite::params![embedding.as_bytes()], |r| r.get(0))
        .map_err(|e| format!("Couldn't run search query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Couldn't read search results: {e}"))?;

    // One lookup per ranked id rather than a second bulk query -- this
    // app's realistic scale (client-side pagination at ~8/page in
    // Sidebar.jsx) never justifies more, same reasoning that keeps
    // `list_projects` a plain `SELECT *` with no server-side paging.
    ranked_ids.iter().map(|id| query_project_by_id(&conn, id)).collect()
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
}
