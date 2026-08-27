// Persisted schedule of future/recurring Instagram posts, ticked once a
// minute from a background task spawned in lib.rs's setup() -- runs
// entirely on Tauri's own tokio runtime, no window needed, so a schedule
// still fires while the app sits tray-only with the window closed.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::instagram;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Recurrence {
    Once,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledPost {
    pub id: String,
    pub caption: String,
    pub recurrence: Recurrence,
    /// Unix seconds. The next time this schedule should fire; advanced by
    /// exactly one day's/week's worth of seconds after each attempt for a
    /// recurring entry -- deliberately plain duration math rather than
    /// calendar/weekday-aware arithmetic, so this stays simple and
    /// dependency-free at the cost of drifting by daylight-saving's ~1 hour
    /// twice a year, an acceptable tradeoff for a personal scheduling tool.
    pub next_run: i64,
    /// Video file paths waiting to be posted by this schedule, oldest
    /// first. A `Once` schedule always starts with exactly one entry,
    /// consumed on its single fire.
    pub queue: Vec<String>,
    pub status: String,
    pub last_result: Option<String>,
    pub created_at: i64,
}

fn schedule_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create app data directory: {e}"))?;
    Ok(dir.join("scheduled-posts.json"))
}

fn load_posts(app: &AppHandle) -> Result<Vec<ScheduledPost>, String> {
    let path = schedule_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|e| format!("Couldn't read saved schedules: {e}")),
        Err(_) => Ok(Vec::new()),
    }
}

fn save_posts(app: &AppHandle, posts: &[ScheduledPost]) -> Result<(), String> {
    let path = schedule_path(app)?;
    let contents = serde_json::to_string(posts).map_err(|e| format!("Couldn't serialize schedules: {e}"))?;
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't write schedules: {e}"))
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn generate_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}")
}

#[tauri::command]
pub fn list_scheduled_posts(app: AppHandle) -> Result<Vec<ScheduledPost>, String> {
    load_posts(&app)
}

/// Creates a new schedule, or -- for `Daily`/`Weekly` -- appends to an
/// existing one with the same recurrence instead of creating a duplicate,
/// since a personal setup normally wants at most one daily and one weekly
/// slot, each fed over time by a growing queue of ready clips.
#[tauri::command]
pub fn schedule_instagram_post(
    app: AppHandle,
    video_path: String,
    caption: String,
    recurrence: Recurrence,
    first_run: i64,
) -> Result<ScheduledPost, String> {
    let mut posts = load_posts(&app)?;

    if recurrence != Recurrence::Once {
        if let Some(existing) = posts.iter_mut().find(|p| p.recurrence == recurrence) {
            existing.queue.push(video_path);
            if existing.status.starts_with("paused") {
                existing.status = "scheduled".to_string();
            }
            let updated = existing.clone();
            save_posts(&app, &posts)?;
            return Ok(updated);
        }
    }

    let post = ScheduledPost {
        id: generate_id(),
        caption,
        recurrence,
        next_run: first_run,
        queue: vec![video_path],
        status: "scheduled".to_string(),
        last_result: None,
        created_at: now_unix_seconds(),
    };
    posts.push(post.clone());
    save_posts(&app, &posts)?;
    Ok(post)
}

#[tauri::command]
pub fn delete_scheduled_post(app: AppHandle, id: String) -> Result<(), String> {
    let mut posts = load_posts(&app)?;
    posts.retain(|p| p.id != id);
    save_posts(&app, &posts)
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn truncate_for_notification(text: &str) -> String {
    if text.chars().count() <= 60 {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(60).collect::<String>())
    }
}

/// Runs once a minute (see `lib.rs`'s scheduler task): finds every due
/// schedule, posts its next queued clip, and advances `next_run` for
/// recurring ones. Attempts run one at a time, in whatever order they're
/// stored, deliberately not concurrently -- both to keep this simple and
/// because publishing already spins up its own temporary tunnel per call,
/// which isn't worth parallelizing for what's expected to be at most a
/// couple of posts a day.
pub async fn run_due_posts(app: &AppHandle) {
    // Best-effort, independent of whether any post is actually due --
    // keeps a connected account's token refreshed with days to spare even
    // during a long stretch with nothing scheduled, so it's still valid
    // whenever the next post (or a manual "Post now") does come along.
    let _ = instagram::refresh_instagram_token_if_needed(app).await;

    let mut posts = match load_posts(app) {
        Ok(p) => p,
        Err(_) => return,
    };
    if posts.is_empty() {
        return;
    }

    let now = now_unix_seconds();
    let mut changed = false;

    for post in posts.iter_mut() {
        if post.next_run > now {
            continue;
        }
        if post.queue.is_empty() {
            if post.status != "paused (queue empty)" {
                post.status = "paused (queue empty)".to_string();
                changed = true;
            }
            continue;
        }

        changed = true;
        let video_path = post.queue.remove(0);
        match instagram::publish_reel(app, &video_path, &post.caption).await {
            Ok(result) => {
                post.status = "posted".to_string();
                post.last_result = Some(format!("Posted (media id {})", result.media_id));
                notify(app, "Posted to Instagram", &truncate_for_notification(&post.caption));
            }
            Err(e) => {
                post.status = "error".to_string();
                post.last_result = Some(e.clone());
                notify(app, "Instagram post failed", &e);
            }
        }

        match post.recurrence {
            Recurrence::Once => {}
            Recurrence::Daily => post.next_run += 24 * 60 * 60,
            Recurrence::Weekly => post.next_run += 7 * 24 * 60 * 60,
        }
    }

    // A `Once` schedule that posted successfully has served its purpose;
    // one that failed stays visible (with its error) so the user can see
    // why, rather than silently vanishing.
    posts.retain(|p| !(p.recurrence == Recurrence::Once && p.status == "posted"));

    if changed {
        let _ = save_posts(app, &posts);
    }
}
