// System tray + "stay running in the background" window lifecycle.
//
// Closing the main window does NOT exit the app -- it destroys the
// WebView entirely (not just hides it) and the process keeps running,
// tray-only, until the tray menu's "Quit" is chosen. Destroying rather
// than hiding is a deliberate memory/CPU tradeoff: a hidden-but-alive
// window still costs WebView2's baseline footprint (100MB+) the whole
// time it sits in the tray, whereas a fully destroyed window leaves only
// the lean Rust process + the scheduler's sleeping tokio task resident.
// The cost is a brief, visible rebuild delay each time the tray icon is
// clicked to reopen -- an explicit, asked-for tradeoff (see the plan this
// was built from), not an oversight.
//
// This is also why `run_pipeline`'s background hardware-encoder-detection
// prefetch and the scheduler (scheduler.rs, added alongside this) are
// designed to depend only on the AppHandle, never on the window existing
// -- they need to keep working while the window is destroyed.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const MAIN_WINDOW_LABEL: &str = "main";

/// Set right before the tray menu's "Quit" calls `app.exit()`, and checked
/// by `lib.rs`'s `RunEvent::ExitRequested` handler. Confirmed the hard
/// way that `app.exit()` does *not* bypass that preventable event the way
/// its docs suggest -- it landed in the same handler as a plain window
/// close, so Quit was silently swallowed by the guard meant only to keep
/// the app alive when the window closes on its own. This flag is what
/// actually distinguishes the two, rather than relying on which API path
/// was assumed to skip the event.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

/// Rebuilds the main window from scratch if it doesn't currently exist
/// (destroyed by a previous close-to-tray), or just focuses it if it
/// does. Same URL/size Tauri.conf.json's original "main" window used --
/// this is a plain recreation, not a different window.
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let Ok(builder) = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::default())
        .title("Reels Caption App")
        .inner_size(700.0, 700.0)
        .resizable(true)
        .build()
    else {
        return;
    };
    let _ = builder.set_focus();
    register_close_to_tray(&builder);
}

/// Intercepts this window's own close button: prevents the default
/// exit-the-app behavior and destroys just this window instead, leaving
/// the tray icon (and the scheduler task behind it) running. Called once
/// for the window built at startup (see `init`) and again for every
/// window `show_main_window` rebuilds, since a freshly built window
/// doesn't inherit a listener registered on a previous, now-destroyed one.
fn register_close_to_tray(window: &tauri::WebviewWindow) {
    let window_clone = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window_clone.destroy();
        }
    });
}

/// Sets up the tray icon + menu and arms the main window's close-to-tray
/// behavior. Called once from `lib.rs`'s `setup()`.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        register_close_to_tray(&window);
    }

    let open_item = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().ok_or(tauri::Error::AssetNotFound(
            "default window icon (needed for the tray icon)".to_string(),
        ))?)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => {
                QUITTING.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } =
                event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
