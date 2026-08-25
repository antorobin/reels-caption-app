// Shared conda-environment discovery, used by every feature that shells
// out to a Python tool inside a named conda env (MFA's "aligner", the
// "media-ai" env). Extracted from mfa.rs once a second conda-based
// feature needed the exact same discovery logic — see its history for the
// original single-env version.

use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Candidate `conda` locations to try, in order, before giving up.
/// Miniconda/Anaconda don't reliably end up on PATH after a default
/// install (often deliberately, to avoid clobbering the system Python),
/// so a plain `Command::new("conda")` relying on PATH resolution alone
/// isn't enough — these mirror where the installer actually put it
/// during setup and testing.
///
/// Also sweeps every other fixed-drive root (D:\, E:\, ...), not just
/// paths under ProgramData/USERPROFILE/SystemDrive: confirmed during
/// setup that a *second*, otherwise-unrelated conda install can easily
/// end up on another drive (e.g. to dodge the `C:\Users\<name(...)>`
/// short-path bug the installer hits with parentheses in the username —
/// see README), and there is no reliable env var pointing at it. Multiple
/// conda installs can coexist on one machine; only one of them may
/// actually have the target env, so every candidate that looks like a
/// conda install has to be checked for real, not just assumed absent.
pub fn conda_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("conda")];
    for var in ["ProgramData", "USERPROFILE", "SystemDrive"] {
        if let Some(base) = std::env::var_os(var) {
            for dist in ["miniconda3", "anaconda3"] {
                candidates.push(PathBuf::from(&base).join(dist).join("Scripts").join("conda.exe"));
            }
        }
    }
    let system_letter = std::env::var_os("SystemDrive")
        .and_then(|d| d.to_string_lossy().chars().next().map(|c| c.to_ascii_uppercase()));
    for letter in b'C'..=b'Z' {
        let letter = letter as char;
        if Some(letter) == system_letter {
            continue; // already covered via SystemDrive above
        }
        for dist in ["miniconda3", "anaconda3"] {
            candidates.push(PathBuf::from(format!("{letter}:\\")).join(dist).join("Scripts").join("conda.exe"));
        }
    }
    candidates
}

/// A `conda.exe` responding to `--version` only proves *that install*
/// works — it says nothing about whether `env_name` (with the tool this
/// feature needs actually installed into it) exists under it. Multiple
/// conda installs can coexist on one machine, so the only real test is
/// running `probe_args` inside the named env via this specific conda
/// binary and seeing if it succeeds.
pub async fn conda_env_has_tool(conda_path: &str, env_name: &str, probe_args: &[&str]) -> bool {
    Command::new(conda_path)
        .args(["run", "-n", env_name])
        .args(probe_args)
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// Finds a conda install with `env_name` set up and probeable via
/// `probe_args` (e.g. `["mfa", "version"]`, or `["python", "-c", "import
/// numpy"]`). Checks `env_var_override` first (lets a user point at a
/// nonstandard install), then every candidate from [`conda_candidates`].
pub async fn resolve_conda_env(env_name: &str, probe_args: &[&str], env_var_override: &str) -> Result<String, String> {
    if let Ok(path) = std::env::var(env_var_override) {
        return Ok(path);
    }
    for candidate in conda_candidates() {
        let is_bare_name = candidate == Path::new("conda");
        if !is_bare_name && !candidate.exists() {
            continue;
        }
        let path_arg = candidate.to_string_lossy().to_string();
        if conda_env_has_tool(&path_arg, env_name, probe_args).await {
            return Ok(path_arg);
        }
    }
    Err(format!(
        "Couldn't find a conda install with the '{env_name}' environment set up. Set it up \
         (see README), or if it's already installed somewhere this couldn't find, set \
         {env_var_override} to that conda executable's path."
    ))
}

/// Resolves `env_name`'s actual root directory (its `sys.prefix`) given an
/// already-working `conda_path` — so callers can invoke that env's own
/// executables *directly* (e.g. `<prefix>\Scripts\whatever.exe`) instead
/// of going through `conda run` for every real call. Worth doing:
/// confirmed directly that this conda install's `run` subcommand can
/// crash outright on certain argument shapes (e.g. a value containing a
/// `/`, as in a Hugging Face model id like `org/model-name`) — a bug in
/// conda's own Windows command-line wrapping, not anything under this
/// app's control. Resolving the prefix once (the caller should cache the
/// result) and calling the env's binaries directly sidesteps that
/// entirely for every subsequent call.
pub async fn resolve_conda_env_prefix(conda_path: &str, env_name: &str) -> Result<PathBuf, String> {
    let output = Command::new(conda_path)
        .args(["run", "-n", env_name, "python", "-c", "import sys; print(sys.prefix)"])
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .await
        .map_err(|e| format!("Failed to resolve '{env_name}' env prefix: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to resolve '{env_name}' env prefix: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if prefix.is_empty() {
        return Err(format!("'{env_name}' env prefix resolved to an empty path"));
    }
    Ok(PathBuf::from(prefix))
}
