// Global-hotkey mic dictation: press the shortcut from anywhere in the OS
// (even with the main window closed/tray-only) to pop up a small floating
// HUD, speak, press it again (or Enter, inside the HUD) to finish -- the
// result updates the currently loaded video's captions the same way
// VoiceoverSection.jsx's "Record from mic" button already does, just
// reachable without opening the main window first.
//
// Deliberately NOT a universal system-wide dictation tool (record -> paste
// into whatever app was previously focused) -- this app has no arbitrary
// text field to insert into; the destination is always the currently
// loaded video's transcript in the main window. If no video is loaded,
// the main window's own listener (App.jsx) surfaces that rather than the
// HUD silently doing nothing.
//
// The HUD is a second, minimal Vite entry point (dictation-hud.html +
// DictationHud.jsx) rather than a route inside the main React app, since
// it's a genuinely separate small window (borderless, always-on-top, no
// taskbar entry) with its own lifecycle -- built fresh on first use and
// reused (shown again) afterward, mirroring tray.rs's own "rebuild on
// demand" pattern for the main window.
//
// Recognition itself reuses `streaming_stt.rs`'s live dictation commands
// (`start_live_dictation`/`stop_live_dictation`) -- the same backend
// LiveDictationPanel.jsx's in-app "Live dictation" panel uses -- so text
// grows here too while you speak, and Tamil+English code-switching works
// the same way, rather than this HUD doing its own separate
// record-then-batch-transcribe pass.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const HUD_WINDOW_LABEL: &str = "dictation-hud";
const HUD_WIDTH: f64 = 280.0;
const HUD_HEIGHT: f64 = 120.0;

/// `Ctrl+Shift+D` -- "D" for Dictate. Reconstructed (not a shared static)
/// wherever it's needed since building a `Shortcut` is cheap and this
/// keeps the registration and the handler's match check trivially in sync.
fn dictation_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyD)
}

/// Shows the HUD, building it fresh the first time and just re-showing it
/// afterward. If it's already open, a second hotkey press means "stop and
/// finish" rather than "open another one" -- relayed as an event so the
/// HUD's own JS (already listening) can tell that apart from the user
/// pressing Escape to cancel instead.
fn toggle_hud(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(HUD_WINDOW_LABEL) {
        let _ = window.emit("dictation-toggle-stop", ());
        return;
    }

    let _ = WebviewWindowBuilder::new(app, HUD_WINDOW_LABEL, WebviewUrl::App("dictation-hud.html".into()))
        .title("Dictate")
        .inner_size(HUD_WIDTH, HUD_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .center()
        .focused(true)
        .build();
}

/// The global-shortcut plugin's handler, wired up in `lib.rs`'s
/// `.plugin(...)` chain (before `setup()` runs, per that plugin's own
/// expected registration point) -- this function is where the actual
/// "what does the hotkey do" logic lives, kept here instead of inline in
/// `lib.rs` so it reads alongside the rest of this feature.
pub fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut, event: tauri_plugin_global_shortcut::ShortcutEvent) {
    if *shortcut == dictation_shortcut() && event.state() == ShortcutState::Pressed {
        toggle_hud(app);
    }
}

/// Registers the shortcut itself -- needs an `AppHandle`, so this runs
/// from `lib.rs`'s `setup()`, after the plugin above is already installed.
///
/// Deliberately infallible: a global hotkey is a shared OS resource, and
/// `register()` fails with "HotKey already registered" whenever another
/// app -- or a second/stale instance of this one -- already holds
/// `Ctrl+Shift+D`. That must not brick startup (it used to panic the whole
/// setup hook). On failure the hotkey HUD just isn't reachable; the in-app
/// "Live dictation" panel still works.
pub fn init(app: &AppHandle) {
    if let Err(e) = app.global_shortcut().register(dictation_shortcut()) {
        eprintln!("dictation: couldn't register the Ctrl+Shift+D global hotkey ({e}); the hotkey HUD is disabled this session. Use the in-app Live dictation panel instead.");
    }
}
