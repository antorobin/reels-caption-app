// Resolves the ffmpeg/ffprobe executables used throughout this crate.
// (Transcription no longer shells out to a bundled binary here — see
// stt.rs, which resolves its own conda environment instead.)
//
// Preference order:
//   1. `REELS_CAPTION_APP_FFMPEG`/`REELS_CAPTION_APP_FFPROBE` env vars, if
//      set (explicit override, handy for testing).
//   2. A binary bundled as an app resource under `resources/bin/` (see
//      README section 6 / resources/bin/README.md) — the "no separate
//      install step" path for a `tauri build` distributable.
//   3. The bare command name, resolved via PATH — the `tauri dev` path on
//      a dev machine that already has ffmpeg installed and hasn't
//      populated resources/bin/ locally.
//
// The bundled-vs-PATH decision is resolved once, at startup (`init`,
// called from `run()`'s setup hook, which is the one place in this crate
// that both runs before any command and has an `AppHandle` on hand) and
// cached — most ffmpeg call sites are plain helper functions several
// layers from any command handler and would otherwise need an
// `AppHandle` threaded through just for this.
//
// The getters additionally check `~/.reels-caption-app/runtime/bin/`
// (a component downloaded by `runtime_fetch.rs` — the slim installer's
// path) *live* on each call, since first-run setup can install ffmpeg
// after `init` has already run. That check is a single `Path::exists()`,
// negligible next to spawning ffmpeg.

use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static FFMPEG_PATH: OnceLock<PathBuf> = OnceLock::new();
static FFPROBE_PATH: OnceLock<PathBuf> = OnceLock::new();
static FONTS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// A downloaded runtime binary at `~/.reels-caption-app/runtime/bin/<stem>`
/// (`.exe` on Windows), if present — takes precedence over a bundled copy.
fn downloaded_bin(stem: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) { format!("{stem}.exe") } else { stem.to_string() };
    let p = crate::runtime_fetch::runtime_dir().join("bin").join(name);
    p.exists().then_some(p)
}

/// Bundled fonts (see resources/fonts/) that the `ass` burn filter points
/// `fontsdir` at, so caption rendering doesn't depend on the right fonts
/// being installed system-wide — same "bundled resource, no separate
/// install step" idea as ffmpeg above, but for a whole directory rather
/// than a single executable, so there's no bare-command PATH fallback:
/// the dev-machine case instead falls back to this crate's own
/// `resources/fonts/` directly (works for `tauri dev` without first
/// needing a `tauri build` to populate the bundled resource dir).
fn resolve_fonts_dir(app: &AppHandle) -> PathBuf {
    if let Ok(path) = std::env::var("REELS_CAPTION_APP_FONTS_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("fonts");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("fonts")
}

fn resolve(app: &AppHandle, env_var: &str, resource_filename: &str, path_fallback: &str) -> PathBuf {
    if let Ok(path) = std::env::var(env_var) {
        return PathBuf::from(path);
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("bin").join(resource_filename);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(path_fallback)
}

/// Must be called once during app setup, before any command that shells
/// out to ffmpeg runs.
pub fn init(app: &AppHandle) {
    let _ = FFMPEG_PATH.set(resolve(app, "REELS_CAPTION_APP_FFMPEG", "ffmpeg.exe", "ffmpeg"));
    let _ = FFPROBE_PATH.set(resolve(app, "REELS_CAPTION_APP_FFPROBE", "ffprobe.exe", "ffprobe"));
    let _ = FONTS_DIR.set(resolve_fonts_dir(app));
}

/// A downloaded `runtime/bin/ffmpeg.exe` wins; otherwise the startup-
/// resolved bundled/PATH value (bare `ffmpeg` if `init` hasn't run, e.g.
/// unit tests).
pub fn ffmpeg_path() -> PathBuf {
    downloaded_bin("ffmpeg").unwrap_or_else(|| FFMPEG_PATH.get_or_init(|| PathBuf::from("ffmpeg")).clone())
}

pub fn ffprobe_path() -> PathBuf {
    downloaded_bin("ffprobe").unwrap_or_else(|| FFPROBE_PATH.get_or_init(|| PathBuf::from("ffprobe")).clone())
}

pub fn fonts_dir() -> PathBuf {
    FONTS_DIR
        .get_or_init(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("fonts"))
        .clone()
}
