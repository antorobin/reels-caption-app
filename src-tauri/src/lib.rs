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
mod instagram;
mod jumpcuts;
mod llm;
mod loudness;
mod media_ai;
mod media_host;
mod mixed_language;
mod model_fetch;
mod pipeline;
mod proc_cleanup;
mod scheduler;
mod segments;
mod slang;
mod stt;
mod tray;
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // The launch-args payload (`Some(vec![...])`) only matters on
        // platforms that re-exec the binary with extra flags to detect an
        // autostart-triggered launch (Windows/Linux don't need this) --
        // `None` is the documented default for "no special handling
        // needed on start".
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            bin_paths::init(app.handle());
            // Order matters: clear out anything orphaned by a *previous*
            // run first, then set up the job object that keeps *this*
            // run's own children from ever doing the same.
            proc_cleanup::kill_orphaned_processes(&[bin_paths::ffmpeg_path(), bin_paths::ffprobe_path()]);
            proc_cleanup::init_kill_on_exit();

            tray::init(app.handle())?;

            // Runs once a minute on Tauri's own tokio runtime for as long
            // as the process lives -- including while the window is closed
            // and the app is tray-only, since this doesn't depend on any
            // window existing. See scheduler.rs's doc comment.
            let scheduler_app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    scheduler::run_due_posts(&scheduler_app_handle).await;
                }
            });

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
            instagram::save_instagram_app_config,
            instagram::has_instagram_app_config,
            instagram::open_external_url,
            instagram::connect_instagram_account,
            instagram::get_connected_instagram_account,
            instagram::disconnect_instagram_account,
            instagram::post_to_instagram_now,
            jumpcuts::remove_silence_and_fillers,
            loudness::normalize_audio,
            model_fetch::check_optional_models,
            model_fetch::optional_models_missing,
            model_fetch::download_optional_models,
            scheduler::list_scheduled_posts,
            scheduler::schedule_instagram_post,
            scheduler::delete_scheduled_post,
            stt::get_supported_languages,
            tts::generate_voiceover,
            util::fingerprint_video_file,
            vosync::compute_voiceover_offset,
            vosync::sync_voice_over,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            // Tauri's own default behavior is to exit the whole app once
            // the last window is gone -- true even for a window this app
            // closed itself via `tray.rs`'s `destroy()` call, not just a
            // user-driven close. Without this, closing the main window
            // silently killed the entire process instead of leaving it
            // running in the tray -- confirmed the hard way: `tray.rs`'s
            // per-window `CloseRequested` handler alone (prevent default +
            // destroy) was not enough.
            //
            // `tray.rs`'s Quit menu item calls `app.exit(0)`, which -- also
            // confirmed the hard way, contrary to what its own docs
            // suggest -- lands in this *same* preventable `ExitRequested`
            // event rather than bypassing it. Unconditionally preventing
            // exit here swallowed real Quit clicks along with ordinary
            // window closes. `tray::QUITTING` is what actually
            // distinguishes the two: only prevent exit when this wasn't a
            // deliberate Quit.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
