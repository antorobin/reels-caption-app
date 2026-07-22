// Entry point for the desktop binary. Mobile targets call `run()`
// directly from lib.rs via the `#[tauri::mobile_entry_point]` macro,
// so this file is desktop-only.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    reels_caption_app_lib::run();
}
