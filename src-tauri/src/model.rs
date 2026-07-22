// Resolves the whisper.cpp GGML model file used for transcription.
//
// Preference order:
//   1. `REELS_CAPTION_APP_WHISPER_MODEL` env var, if set (explicit override,
//      handy for testing with a different model without rebuilding).
//   2. A model bundled as an app resource under `resources/models/` (see
//      README section 6 — this is the "no separate download step" path).
//      Tries the quantized filename first, then falls back to the plain
//      f16 filename for anyone with an older resources folder.
//   3. `~/.reels-caption-app/models/` (same two filenames) — a per-user
//      cache location, in case you'd rather fetch the model at first run
//      instead of shipping it inside the installer.
//
// This scaffold does NOT ship a real model binary (it's tens of MB and not
// something to vendor blind) — you still need to drop a `ggml-*.bin` file
// into `src-tauri/resources/models/` yourself. Until you do, this fails
// with a clear, actionable error instead of a crash, matching the rest of
// this scaffold's philosophy (see check_ffmpeg/check_whisper in lib.rs).

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

// q5_0-quantized rather than the original f16 model: benchmarked on this
// project (55s of audio, 8 threads) at ~9.34s vs ~9.90s for f16 — a modest
// but real ~6% speedup, AND the transcript came out byte-for-byte
// identical, so there's no accuracy tradeoff to weigh. (For contrast,
// tiny.en benchmarked ~32% faster but with visibly worse transcription —
// dropped punctuation and a garbled repeated phrase — so that's offered as
// an opt-in tradeoff, not a default.) Quantizing also shrinks the bundled
// download from ~148MB to ~55MB. Re-quantize via `whisper-quantize.exe
// ggml-base.en.bin ggml-base.en-q5_0.bin q5_0` if you swap in a different
// base model.
const BUNDLED_MODEL_FILENAMES: &[&str] = &["ggml-base.en-q5_0.bin", "ggml-base.en.bin"];

fn user_cache_model_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".reels-caption-app").join("models"))
}

pub fn resolve_whisper_model(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("REELS_CAPTION_APP_WHISPER_MODEL") {
        let path = PathBuf::from(path);
        return if path.exists() {
            Ok(path)
        } else {
            Err(format!(
                "REELS_CAPTION_APP_WHISPER_MODEL is set to {} but that file doesn't exist.",
                path.display()
            ))
        };
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        let models_dir = resource_dir.join("models");
        for filename in BUNDLED_MODEL_FILENAMES {
            let path = models_dir.join(filename);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    if let Some(dir) = user_cache_model_dir() {
        for filename in BUNDLED_MODEL_FILENAMES {
            let path = dir.join(filename);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(format!(
        "No whisper.cpp model found. Put one of [{}] in \
         src-tauri/resources/models/ (see README section 6), or set the \
         REELS_CAPTION_APP_WHISPER_MODEL environment variable to a model \
         file's full path.",
        BUNDLED_MODEL_FILENAMES.join(", ")
    ))
}
