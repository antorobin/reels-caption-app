use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

/// A collision-resistant path in the OS temp dir, e.g.
/// `<tmp>/reels-caption-app-audio-<nanos>.wav`.
pub fn unique_temp_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("reels-caption-app-{prefix}-{nanos}.{ext}"))
}

/// Renders a path as a plain string for handing to an external CLI process.
///
/// Windows APIs (notably path canonicalization, which `resource_dir()`
/// goes through internally) can produce `\\?\`-prefixed ("verbatim"/
/// extended-length) paths. That prefix is meant for direct Win32 API
/// consumption. Passed as a plain command-line argument to a MinGW-built
/// binary (like our bundled ffmpeg build), the literal `?` triggers MinGW's
/// CRT argv wildcard-expansion (glob) logic by default and mangles the
/// path into garbage. Stripping the prefix here avoids that whole class
/// of bug for every subprocess call in this crate.
pub fn cli_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(stripped) => stripped.to_string(),
        None => s.into_owned(),
    }
}

/// A cheap signature of a video file's on-disk content (size + modified
/// time), taken right after transcribing it. The app has no other way to
/// tell if a file at the same path got replaced (re-recorded/re-exported
/// over the same filename) between transcribing and burning — without
/// this check, burning would silently use stale word timestamps against
/// different audio, producing captions that drift out of sync with the
/// actual speech.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VideoFingerprint {
    pub size: u64,
    pub modified_ms: i64,
}

fn fingerprint_video(video_path: &str) -> Result<VideoFingerprint, String> {
    let meta = std::fs::metadata(video_path).map_err(|e| format!("Couldn't read video file: {e}"))?;
    let modified_ms = meta
        .modified()
        .map_err(|e| format!("Couldn't read video file's modified time: {e}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Video file's modified time is before the Unix epoch: {e}"))?
        .as_millis() as i64;
    Ok(VideoFingerprint { size: meta.len(), modified_ms })
}

#[tauri::command]
pub fn fingerprint_video_file(video_path: String) -> Result<VideoFingerprint, String> {
    fingerprint_video(&video_path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TranscriptCacheEntry {
    fingerprint: VideoFingerprint,
}

/// Cache file recording "the last time this exact video_path was
/// transcribed, its fingerprint was X" — one file per video path (hashed
/// into the filename), stored in the app's persistent data directory.
///
/// This is deliberately a *disk-persisted* record, not just the in-memory
/// React state the frontend already tracks — this app gets used across
/// separate sessions/processes (one task transcribes a video, a different
/// later task burns it), and a check that only lives in one running
/// session's memory silently stops protecting against exactly the
/// mismatch it exists to catch the moment a new process starts. Grounding
/// the check in the file's own on-disk history instead makes it hold
/// regardless of which process or session calls `burn_captions`.
fn transcript_cache_path(app: &AppHandle, video_path: &str) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    video_path.hash(&mut hasher);
    let key = hasher.finish();
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create app data directory: {e}"))?;
    Ok(dir.join(format!("transcript-cache-{key:x}.json")))
}

/// Records that `video_path`, at its current on-disk fingerprint, was just
/// freshly transcribed. Called after transcription (or after silence/
/// filler-word removal, whose output is an equally-fresh new file).
pub fn record_fresh_transcript(app: &AppHandle, video_path: &str) -> Result<(), String> {
    let fingerprint = fingerprint_video(video_path)?;
    let path = transcript_cache_path(app, video_path)?;
    let contents = serde_json::to_string(&TranscriptCacheEntry { fingerprint })
        .map_err(|e| format!("Couldn't serialize transcript cache entry: {e}"))?;
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't write transcript cache: {e}"))
}

/// Verifies `video_path`'s on-disk content hasn't changed since it was
/// last recorded as transcribed via [`record_fresh_transcript`]. Called at
/// the top of `burn_captions`, before any encoding work starts.
pub fn verify_transcript_is_fresh(app: &AppHandle, video_path: &str) -> Result<(), String> {
    let current = fingerprint_video(video_path)?;
    let path = transcript_cache_path(app, video_path)?;
    let contents = std::fs::read_to_string(&path).map_err(|_| {
        "This video hasn't been transcribed yet in a way this app can verify — run the transcription pipeline on this exact file before burning.".to_string()
    })?;
    let entry: TranscriptCacheEntry =
        serde_json::from_str(&contents).map_err(|e| format!("Couldn't read transcript cache: {e}"))?;
    if entry.fingerprint != current {
        return Err(
            "This video file has changed since it was transcribed (re-recorded, re-exported, or edited outside this app, possibly in a different session) — the transcript no longer matches its audio. Re-run the transcription pipeline on this exact file before burning.".to_string(),
        );
    }
    Ok(())
}

/// Payload for progress events emitted to the frontend while a long-running
/// job (transcription, caption burning) runs in the background — lets the
/// UI show a progress bar/stage label instead of freezing.
#[derive(Clone, Serialize)]
pub struct ProgressPayload {
    pub stage: String,
    pub percent: Option<f64>,
    pub message: Option<String>,
}

pub fn emit_progress(app: &AppHandle, event: &str, stage: &str, percent: Option<f64>, message: Option<String>) {
    let _ = app.emit(
        event,
        ProgressPayload {
            stage: stage.to_string(),
            percent,
            message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_path_strips_windows_verbatim_prefix() {
        let path = Path::new(r"\\?\D:\projects\app\model.bin");
        assert_eq!(cli_path(path), r"D:\projects\app\model.bin");
    }

    #[test]
    fn cli_path_leaves_normal_paths_unchanged() {
        let path = Path::new(r"D:\projects\app\model.bin");
        assert_eq!(cli_path(path), r"D:\projects\app\model.bin");
    }
}
