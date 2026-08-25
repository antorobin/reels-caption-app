// This is the shared core of the app. Both the desktop binary (main.rs)
// and the mobile targets (iOS/Android) call `run()` from here.
//
// IMPORTANT MOBILE NOTE:
// `check_ffmpeg` below uses `std::process::Command` to call an external
// binary. This works fine on macOS, Windows, and Linux, where ffmpeg is
// installed on the system or bundled as a "sidecar" executable.
//
// On iOS and Android, spawning subprocesses / running arbitrary CLI
// binaries is NOT allowed by the OS sandbox. To make this feature work
// on mobile you'd need to compile ffmpeg as a static/dynamic library
// (e.g. via ffmpeg-kit for mobile) and write Rust FFI bindings to call
// it in-process instead of as a subprocess, swapping the body of this
// command when compiled for `target_os = "ios"` or `target_os =
// "android"`.
//
// Transcription (see stt.rs) is conda/Python-based and was never
// mobile-compatible to begin with — no FFI path exists for it today.
//
// The command signatures and the React UI stay identical either way —
// only the implementation inside each command changes per platform.

mod bin_paths;
mod captions;
mod conda_util;
mod content_ideas;
mod diarize;
mod ducking;
mod ffmpeg;
mod jumpcuts;
mod llm;
mod loudness;
mod media_ai;
mod pipeline;
mod proc_cleanup;
mod segments;
mod slang;
mod stt;
mod tts;
mod util;
mod voice_clone;
mod vosync;

use std::process::Command;
use tauri::Manager;

/// Called by the frontend once it's actually mounted and painted (see
/// `App.jsx`'s startup effect) — swaps the splashscreen window (shown
/// natively the instant the process starts, so there's no blank-window
/// flash while the webview spins up) for the real main window, rather
/// than showing the main window immediately and letting the user watch
/// it render.
#[tauri::command]
fn close_splashscreen(app: tauri::AppHandle) {
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! This message came from Rust. 🦀", name)
}

#[tauri::command]
fn check_ffmpeg() -> Result<String, String> {
    // Desktop: shells out to the bundled (or PATH-resolved) ffmpeg binary
    // — see bin_paths.rs.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let output = Command::new(bin_paths::ffmpeg_path())
            .arg("-version")
            .output()
            .map_err(|e| format!("ffmpeg not found: {}", e))?;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            bin_paths::init(app.handle());
            // Order matters: clear out anything orphaned by a *previous*
            // run first, then set up the job object that keeps *this*
            // run's own children from ever doing the same.
            proc_cleanup::kill_orphaned_processes(&[bin_paths::ffmpeg_path(), bin_paths::ffprobe_path()]);
            proc_cleanup::init_kill_on_exit();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            close_splashscreen,
            greet,
            check_ffmpeg,
            pipeline::run_pipeline,
            pipeline::transcribe_audio_file,
            captions::burn_captions,
            captions::analyze_prosody,
            content_ideas::generate_content_ideas,
            diarize::diarize_speakers,
            ducking::duck_music,
            jumpcuts::remove_silence_and_fillers,
            loudness::normalize_audio,
            stt::get_supported_languages,
            tts::generate_voiceover,
            util::fingerprint_video_file,
            vosync::compute_voiceover_offset,
            vosync::sync_voice_over,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
