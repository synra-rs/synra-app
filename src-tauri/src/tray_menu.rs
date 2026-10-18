//! What the tray icon's menu items mean, without a window.
//!
//! # Why this is apart from [`crate::tray`]
//!
//! Because a menu id is a string and a [`TrayAction`] is a decision, and neither needs Tauri — which means this mapping can
//! be asserted by the build that runs the tests. The tray itself needs an event loop and an icon, so it lives behind
//! `tauri-shell` and is wired rather than tested; the part where a typo would be silent (an id nobody matches, so a click
//! does nothing) is the part asserted here.

/// The menu item that brings the window back.
pub const SHOW: &str = "tray.show";
/// The menu item that ends the process.
pub const QUIT: &str = "tray.quit";

/// What a tray interaction asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show, unminimise and focus the main window.
    Show,
    /// Exit the application.
    Quit,
}

/// The action a menu id stands for, or `None` for an id this build does not know.
///
/// `None` is an answer rather than a panic because the event may come from a tray that outlived a newer build's menu, or
/// from a platform that answers with an id nobody set — and a tray click must never be able to kill the process by
/// accident.
#[must_use]
pub fn action_for(id: &str) -> Option<TrayAction> {
    if id == SHOW {
        Some(TrayAction::Show)
    }
    else if id == QUIT {
        Some(TrayAction::Quit)
    }
    else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{SHOW, TrayAction, QUIT, action_for};

    #[test]
    fn the_two_menu_items_map_to_their_actions() {
        assert_eq!(action_for(SHOW), Some(TrayAction::Show));
        assert_eq!(action_for(QUIT), Some(TrayAction::Quit));
    }

    #[test]
    fn an_identifier_this_build_did_not_set_is_an_answer_rather_than_a_panic() {
        // ★ A tray click must not be able to end the process because a menu id changed, and the caller has to be able to
        // tell "nothing to do" from "quit".
        assert_eq!(action_for("tray.something-else"), None);
        assert_eq!(action_for(""), None);
        assert_eq!(action_for("quit"), None);
    }
}
