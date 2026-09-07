// Resolves the Python environment for each "shell out to a Python tool"
// feature (stt / tts / media-ai / voice-clone), preferring a *relocatable
// Python* shipped inside the app over a system `conda` install.
//
// This is the piece that lets a plain MSI install "just work" with no
// per-machine conda setup: `scripts/build-python-runtime.mjs` builds one
// self-contained env per feature under `src-tauri/resources/python/<env>/`
// (a python-build-standalone interpreter + that feature's pinned wheels),
// `tauri.conf.json`'s `bundle.resources` ships them, and this module finds
// them at runtime.
//
// Resolution order, first hit wins:
//   1. `<env_var_override>` (e.g. REELS_CAPTION_APP_STT_CONDA_PATH) -- a
//      hard override. Accepts either an env *prefix* directory (one that
//      has `python.exe` / `bin/python3` in it) or, unchanged from before
//      this module existed, a `conda(.exe)` binary path.
//   2. A bundled relocatable env: `<resources>/python/<env_name>/`, via
//      Tauri's packaged resource dir in a real build, or
//      `<CARGO_MANIFEST_DIR>/resources/python/<env_name>/` for `tauri dev`
//      once the build script has populated it locally.
//   3. System conda discovery (`conda_util`) -- the original behaviour,
//      untouched, for a dev machine that hasn't built the bundled runtime.
//
// Callers get an env *prefix* back and invoke `<prefix>/python(.exe)`
// directly. Never `conda run`: this project has hit real command-line-
// wrapping crashes in `conda run` (a `/` in an argument, e.g. a Hugging
// Face model id) -- see conda_util.rs. stt.rs / tts.rs / voice_clone.rs
// already call the env's python directly; media_ai.rs is switched onto the
// same pattern as part of this.
//
// Resolved base dir is cached at startup (`init`, from `run()`'s setup
// hook, alongside `bin_paths::init`) so the per-feature resolvers stay
// `AppHandle`-free, mirroring bin_paths.rs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

/// The directory holding one subdir per bundled relocatable env
/// (`python/stt`, `python/tts`, `python/media-ai`, `python/voice-clone`).
/// `Some(dir)` when a bundled runtime is present, `None` when it isn't --
/// in which case every resolve falls through to the conda path exactly as
/// before. Wrapped in an outer `OnceLock` so "init has run, found nothing"
/// is distinct from "init hasn't run yet."
static BUNDLED_PYTHON_BASE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Must be called once during app setup, before any command that shells
/// out to a Python tool runs. Safe to call without a bundled runtime
/// present -- it just records that none was found.
pub fn init(app: &AppHandle) {
    let _ = BUNDLED_PYTHON_BASE.set(resolve_bundled_base(app));
}

fn resolve_bundled_base(app: &AppHandle) -> Option<PathBuf> {
    // A real `tauri build`: resources land under the packaged resource dir.
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("python");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    // `tauri dev` / `cargo run`: the resource dir doesn't point at
    // `src-tauri/resources/`, so look there directly. Only exists once
    // `scripts/build-python-runtime.mjs` has been run on this machine.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("python");
    dev.is_dir().then_some(dev)
}

/// `<prefix>/python.exe` on Windows, `<prefix>/bin/python3` elsewhere --
/// the layout both conda envs and python-build-standalone use.
pub fn python_exe(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("python.exe")
    } else {
        prefix.join("bin").join("python3")
    }
}

fn bundled_env_prefix(env_name: &str) -> Option<PathBuf> {
    let base = BUNDLED_PYTHON_BASE.get()?.as_ref()?;
    let prefix = base.join(env_name);
    python_exe(&prefix).exists().then_some(prefix)
}

/// Resolves the env *prefix* (its `sys.prefix` root, where `python.exe`
/// lives) for one feature's environment. See the module comment for the
/// full order. `probe_args` is only used on the conda fallback path (the
/// bundled env is a single known-complete tree -- if its `python.exe` is
/// there, it's trusted).
pub async fn resolve_env_prefix(
    env_name: &str,
    probe_args: &[&str],
    env_var_override: &str,
) -> Result<PathBuf, String> {
    if let Ok(val) = std::env::var(env_var_override) {
        let as_prefix = PathBuf::from(&val);
        if python_exe(&as_prefix).exists() {
            return Ok(as_prefix);
        }
        // Back-compat: the override was set to a `conda(.exe)` path, the
        // only thing it could point at before bundled runtimes existed.
        return crate::conda_util::resolve_conda_env_prefix(&val, env_name).await;
    }

    if let Some(prefix) = bundled_env_prefix(env_name) {
        return Ok(prefix);
    }

    let conda = crate::conda_util::resolve_conda_env(env_name, probe_args, env_var_override).await?;
    crate::conda_util::resolve_conda_env_prefix(&conda, env_name).await
}
