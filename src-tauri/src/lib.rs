// This is the shared core of the app. Both the desktop binary (main.rs)
// and the mobile targets (iOS/Android) call `run()` from here.
//
// IMPORTANT MOBILE NOTE:
// `check_ffmpeg` and `check_whisper` below use `std::process::Command` to
// call external binaries. This works fine on macOS, Windows, and Linux,
// where ffmpeg/whisper.cpp are installed on the system or bundled as
// "sidecar" executables.
//
// On iOS and Android, spawning subprocesses / running arbitrary CLI
// binaries is NOT allowed by the OS sandbox. To make this feature work
// on mobile you'd need to:
//   1. Compile ffmpeg and whisper.cpp as static/dynamic libraries
//      (e.g. via ffmpeg-kit for mobile, and whisper.cpp's C API).
//   2. Write Rust FFI bindings (or use existing crates like `whisper-rs`)
//      to call them as in-process functions instead of subprocesses.
//   3. Swap the body of these commands to call those bindings when
//      compiled for `target_os = "ios"` or `target_os = "android"`.
//
// The command signatures and the React UI stay identical either way —
// only the implementation inside each command changes per platform.

mod captions;
mod ffmpeg;
mod jumpcuts;
mod model;
mod pipeline;
mod segments;
mod util;

use std::process::Command;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! This message came from Rust. 🦀", name)
}

#[tauri::command]
fn check_ffmpeg() -> Result<String, String> {
    // Desktop: shells out to the system/bundled ffmpeg binary.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let output = Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map_err(|e| format!("ffmpeg not found on PATH: {}", e))?;

        let text = String::from_utf8_lossy(&output.stdout);
        let first_line = text.lines().next().unwrap_or("ffmpeg found, version unknown");
        Ok(first_line.to_string())
    }

    // Mobile: subprocess calls aren't allowed. Replace this with an FFI
    // call into a bundled ffmpeg library (e.g. ffmpeg-kit) once you add
    // mobile support for real processing.
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        Ok("ffmpeg check is a no-op on mobile in this scaffold — wire up ffmpeg-kit FFI here.".to_string())
    }
}

#[tauri::command]
fn check_whisper() -> Result<String, String> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        // Expects a `whisper-cli` (or `main`, depending on your whisper.cpp
        // build) binary available on PATH, or bundled as a Tauri "sidecar".
        // See README for how to build whisper.cpp and point this at it.
        let output = Command::new("whisper-cli")
            .arg("--help")
            .output();

        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let first_line = text.lines().next().unwrap_or("whisper-cli found");
                Ok(first_line.to_string())
            }
            Err(e) => Err(format!(
                "whisper-cli not found on PATH ({}). Build whisper.cpp and add it to PATH — see README.",
                e
            )),
        }
    }

    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        Ok("whisper check is a no-op on mobile in this scaffold — wire up whisper-rs FFI here.".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            check_ffmpeg,
            check_whisper,
            pipeline::run_pipeline,
            captions::burn_captions,
            jumpcuts::remove_silence_and_fillers,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
