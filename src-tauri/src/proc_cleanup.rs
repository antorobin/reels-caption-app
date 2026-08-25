// Makes sure ffmpeg/stt/media-ai child processes never outlive this
// app. Confirmed the hard way during development: restarting `tauri dev`
// mid-transcription left a bundled CLI process running in the
// background indefinitely — Windows does not kill a process's children
// when the process itself is killed, so every restart during a long job
// silently accumulated orphaned processes, each still burning real CPU,
// competing with whatever the next run started.
//
// Two independent halves:
//   1. `init_kill_on_exit` — a Windows Job Object with
//      `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigned to this process
//      itself (not to each child individually). By default, without a
//      breakaway flag, every process this one spawns automatically
//      becomes a member of the same job too, at any depth — Windows'
//      normal job-inheritance behavior. Once the *last* handle to the job
//      closes, every member process is killed. Rather than track that
//      handle in a static and worry about when to close it, this
//      deliberately leaks it via `mem::forget`: the OS closes it — and so
//      triggers the kill — automatically the moment this process
//      terminates, by *any* means (normal exit, crash, a dev-server
//      restart forcibly killing the old binary). That's exactly the
//      "clean up no matter how we die" behavior we want, and it needs no
//      changes at any of the many places in this crate that shell out to
//      ffmpeg/stt/conda.
//   2. `kill_orphaned_processes` — a startup backstop for processes that
//      were *already* orphaned before this fix existed (or in the
//      unlikely case something ever escapes the job). Only ever kills
//      processes whose full executable path exactly matches this app's
//      own bundled/resolved ffmpeg/ffprobe binaries — deliberately not
//      matched by process name alone, since "ffmpeg.exe" is common enough
//      that another, unrelated tool's process could easily collide and
//      get killed by mistake. WhisperX runs via a conda environment, not
//      a bundled/resolved path, so it isn't covered by this backstop —
//      only the Job Object above (which covers every child regardless of
//      how it's invoked) protects against an orphaned stt-engine process.
//
// Windows-only (Job Objects are a Windows-specific mechanism) — a no-op
// on other platforms, same as this crate's other platform-gated pieces
// (see check_ffmpeg in lib.rs). Not a correctness gap for this app
// today, since it currently only ships/targets Windows.

#[cfg(windows)]
pub fn init_kill_on_exit() {
    use win32job::{ExtendedLimitInfo, Job};

    let job = match Job::create() {
        Ok(job) => job,
        Err(e) => {
            eprintln!("proc_cleanup: couldn't create job object, child processes won't be auto-killed on exit: {e}");
            return;
        }
    };

    let mut info = ExtendedLimitInfo::new();
    info.limit_kill_on_job_close();
    if let Err(e) = job.set_extended_limit_info(&info) {
        eprintln!("proc_cleanup: couldn't configure job object: {e}");
        return;
    }
    if let Err(e) = job.assign_current_process() {
        eprintln!("proc_cleanup: couldn't assign this process to its job object: {e}");
        return;
    }

    // Deliberately leaked — see module doc comment for why.
    std::mem::forget(job);
}

#[cfg(not(windows))]
pub fn init_kill_on_exit() {}

/// Kills any already-running process whose full executable path matches
/// one of `paths` — best-effort, never fails the caller (startup cleanup
/// shouldn't be able to block the app from launching).
#[cfg(windows)]
pub fn kill_orphaned_processes(paths: &[&std::path::Path]) {
    use std::process::Command;

    let canonical: Vec<String> = paths
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .map(|p| crate::util::cli_path(&p).to_lowercase())
        .collect();
    if canonical.is_empty() {
        return;
    }

    // PowerShell + CIM rather than `taskkill /IM <name>`: this matches on
    // the process's *full executable path*, not just its name, so it
    // can't collide with some unrelated tool that happens to also be
    // called ffmpeg.exe.
    let script = r#"
Get-CimInstance Win32_Process | ForEach-Object {
    if ($_.ExecutablePath) {
        Write-Output ("{0}`t{1}" -f $_.ProcessId, $_.ExecutablePath)
    }
}
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();

    let Ok(output) = output else { return };
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let Some((pid, exe_path)) = line.split_once('\t') else { continue };
        let exe_path = exe_path.trim().to_lowercase();
        if canonical.iter().any(|c| *c == exe_path) {
            let _ = Command::new("taskkill").args(["/F", "/PID", pid.trim()]).output();
        }
    }
}

#[cfg(not(windows))]
pub fn kill_orphaned_processes(_paths: &[&std::path::Path]) {}
