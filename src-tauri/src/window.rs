//! The one window, brought forward by whoever needs it.
//!
//! # Why this is not part of the tray
//!
//! Because two things ask for it and neither owns it: the tray's Show item, and a second launch that found this process
//! already running. A function inside `tray` would make the single-instance path depend on the tray module, which is exactly
//! the kind of coupling that later reads as "the window cannot be shown without a tray".

use tauri::Manager as _;

/// Shows, unminimises and focuses the main window.
///
/// Each step is best-effort: a platform that cannot focus a window still gets it shown, and the alternative is a click
/// that does nothing at all.
pub fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
