use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

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
/// binary (like our whisper-cli build), the literal `?` triggers MinGW's
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
