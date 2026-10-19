//! The `synra://` links this shell understands, and what they become.
//!
//! # Why the parser is separate from the handler
//!
//! Because "which URLs are a share" is a decision that can be tested without a window, an event loop or a deep-link
//! registration. The handler below adds the two side effects — dispatch, and showing the window — and nothing else.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use synra_ipc::dto::IntentSourceDto;
use tauri::Manager as _;

use crate::state::AppState;

/// How long the same URL is treated as the cold-start duplicate it usually is.
///
/// The plugin can deliver a launch URL twice — once through `on_open_url` and once through `get_current` — and both
/// happen within milliseconds. A person clicking the same link twice is not doing it inside this window.
const DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Recent {
    seen: HashMap<String, Instant>,
}

impl Recent {
    /// Whether this URL should be handled now, recording it when it should.
    fn accept(&mut self, url: &str, now: Instant) -> bool {
        self.seen.retain(|_, at| now.duration_since(*at) < DUPLICATE_WINDOW);
        if self.seen.contains_key(url) {
            return false;
        }
        self.seen.insert(url.to_owned(), now);
        true
    }
}

fn recent() -> &'static Mutex<Recent> {
    static RECENT: OnceLock<Mutex<Recent>> = OnceLock::new();
    RECENT.get_or_init(|| Mutex::new(Recent::default()))
}

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
        let fresh = recent()
            .lock()
            .map(|mut recent| recent.accept(url.as_str(), Instant::now()))
            .unwrap_or(true);
        if !fresh {
            continue;
        }
        if let Some(text) = share_text(url.as_str()) {
            let _ = crate::commands::intent::intent_dispatch(
                &state,
                &IntentSourceDto::Share { text, title: None },
            );
        }
        else if url.scheme() == "synra" {
            // ★ A link this build does not know is still a link the user clicked. Focusing the window silently would
            // make it look as if something happened; the notice says what did not.
            state.publish(synra_ipc::event::KernelEvent::Notice {
                message: "notice.unknownLink".to_owned(),
                params: serde_json::json!({ "url": url.as_str() }),
            });
        }
    }
    // ★ After the dispatch: the card the user is about to see is the one the share created.
    crate::window::show_main(app);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Recent, share_text};

    #[test]
    fn the_same_link_is_handled_once_per_cold_start() {
        // ★ `get_current` and `on_open_url` can both deliver a launch URL within milliseconds. Two intents from one click
        // is a bug the user sees as a duplicate card; the window is short enough that a deliberate second click still
        // gets through.
        let mut recent = Recent::default();
        let start = Instant::now();

        assert!(recent.accept("synra://share?text=hi", start));
        assert!(!recent.accept("synra://share?text=hi", start + Duration::from_millis(1)));
        assert!(
            recent.accept("synra://share?text=hi", start + Duration::from_secs(3)),
            "a later click is a new share"
        );
    }

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
