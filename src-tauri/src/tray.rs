//! The tray icon: the window can be put away and brought back without ending the process.
//!
//! # Why a desktop shell needs one
//!
//! Because closing the window is the ordinary way to put an application away, and a synchronous kernel — a listener on a
//! port, transfers in flight — should not be ended by the same gesture that tidies a window. The tray is the second way
//! back to the window and the deliberate way out; without it, "close" and "quit" are one action and a user who only wanted
//! the window out of the way stops every transfer.
//!
//! # What is deliberately not here
//!
//! No "start at login", no badge, no menu beyond two items. Each of those is a preference a user has to be able to find
//! and unset, and a tray menu is the worst place to explain one. The window's own settings page is where such a choice
//! belongs, when there is one to make.
//!
//! # The mapping is elsewhere
//!
//! [`crate::tray_menu`] holds the ids and what they mean, because that part can be tested without an event loop. This
//! module is the wiring, and its failures are reported rather than raised: a machine whose tray is unavailable (some Linux
//! desktops without an indicator host) must still get its window.

use tauri::Manager as _;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::tray_menu::{QUIT, SHOW, TrayAction, action_for};

/// Installs the tray icon and its two items.
///
/// # Errors
///
/// [`tauri::Error`] when the platform refuses the icon or the menu. The caller reports it and carries on: the window is
/// the application, and a tray that could not be created is a convenience missing rather than a reason not to start.
pub fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW, "Show Synra", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("synra")
        .tooltip("Synra")
        .menu(&menu)
        // The menu is the tray's job; a left click is the quick way back to the window. A platform that shows the menu on
        // left click would make the quick way a menu, which is the opposite of the point.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match action_for(event.id().as_ref()) {
            Some(TrayAction::Show) => show_window(app),
            Some(TrayAction::Quit) => {
                // ★ Ask the listener to stop before the process ends. The exit would release the port anyway, but a
                // restart that races the old process for its own port is exactly what the token exists to prevent — and
                // asking first is the whole of "graceful" on this side.
                if let Some(managed) = app.try_state::<Option<synra_core::supervisor::Cancellation>>()
                    && let Some(cancel) = managed.inner().as_ref()
                {
                    cancel.cancel();
                }
                app.exit(0);
            }
            None => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    // The bundle's own icon, so the tray and the window agree about what application this is. A build with no icon still
    // gets a tray; the platform draws its placeholder, which is worse than the icon and better than no way back.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let _tray = builder.build(app)?;
    Ok(())
}

/// Brings the main window back, whether it was hidden, minimised or behind something else.
fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Each step is best-effort: a platform that cannot focus a window still gets it shown, and the alternative is a
        // tray click that does nothing at all.
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
