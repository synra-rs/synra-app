//! The `synra://` links this shell understands, and what they become.
//!
//! # Why the parser is separate from the handler
//!
//! Because "which URLs are a share" is a decision that can be tested without a window, an event loop or a deep-link
//! registration. The handler below adds the two side effects — dispatch, and showing the window — and nothing else.

use std::sync::Arc;

use synra_ipc::dto::IntentSourceDto;
use tauri::Manager as _;

use crate::state::AppState;

/// The text a `synra://share` link carries, or `None` for anything this build does not understand.
///
/// Accepted forms:
///
/// * `synra://share?text=<text>`
/// * `synra://share?url=<text>` — a link is text too, and the name says what the sender meant.
///
/// The query pair is percent-decoded by the URL parser. Unknown hosts and malformed URLs are `None` rather than an empty
/// share: a link that opens the window and does nothing is worse than one that does not open it.
#[must_use]
pub fn share_text(url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    if parsed.scheme() != "synra" || parsed.host_str() != Some("share") {
        return None;
    }
    for (key, value) in parsed.query_pairs() {
        if key == "text" || key == "url" {
            let text = value.trim().to_owned();
            return (!text.is_empty()).then_some(text);
        }
    }
    None
}

/// Dispatches every share a deep link carried, then brings the window forward.
///
/// A link whose state is not there yet is dropped rather than retried: the window is starting up, and a retry queue for
/// a cold-start event would need an owner and a lifetime for a case the next launch fixes.
pub fn receive(app: &tauri::AppHandle, urls: Vec<tauri::Url>) {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };
    for url in urls {
        if let Some(text) = share_text(url.as_str()) {
            let _ = crate::commands::intent::intent_dispatch(
                &state,
                &IntentSourceDto::Share { text, title: None },
            );
        }
    }
    // ★ After the dispatch: the card the user is about to see is the one the share created.
    crate::window::show_main(app);
}

#[cfg(test)]
mod tests {
    use super::share_text;

    #[test]
    fn a_share_link_carries_its_text() {
        assert_eq!(
            share_text("synra://share?text=hello%20there"),
            Some("hello there".to_owned()),
            "percent-encoded text is decoded by the parser, not by a second implementation here"
        );
        assert_eq!(
            share_text("synra://share?url=https%3A%2F%2Fexample.test%2Fa"),
            Some("https://example.test/a".to_owned())
        );
    }

    #[test]
    fn what_is_not_a_share_is_refused_rather_than_opening_an_empty_one() {
        assert_eq!(share_text("synra://share?text="), None, "an empty share is not a share");
        assert_eq!(share_text("synra://share?other=value"), None);
        assert_eq!(share_text("https://example.test/share?text=hi"), None, "the scheme is part of the contract");
        assert_eq!(share_text("synra://other?text=hi"), None);
        assert_eq!(share_text("not a url"), None);
    }
}
