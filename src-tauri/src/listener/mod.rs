//! The shell's own listener: the half that makes this device reachable.
//!
//! # Why the shell listens at all
//!
//! Because it is a host in its own right — the same reason [`crate::discovery`] scans rather than asking the daemon. The two keep
//! **separate data directories**, so a connection the daemon accepts writes into the daemon's tables, and the window would go on
//! showing a device list that never learns what arrived. A desktop that needed a second program running beside it would be one
//! whose transfers only work while somebody starts that program.
//!
//! # What it does with what arrives
//!
//! The finished callback is the kernel's [`record_received`](synra_core::plugin_sync::record_received) — the same function
//! the daemon runs: it writes the history row, records what the peer holds, and inserts the installed row. It does **not**
//! verify or unpack the package: the transfer proved the digest, and the package format (a manifest and a signature over a
//! directory) does not exist yet. What matters here is that the window answers the sender with a `SyncResult`, which the
//! daemon has always done and this shell did not.
//!
//! # Why it runs in its own thread
//!
//! Because `serve` loops until its cancellation is asked to stop: it must not own the thread that draws the window, and it must be
//! stoppable without killing the process.
//!
//! # What the token is and is not for
//!
//! It is for an **in-process** stop — a test, or a future reload — because a process that exits releases its port by itself. Nothing
//! calls `cancel` today: the token is managed so that something can, and the port is released when the window closes because the
//! process ends. A comment that claimed otherwise would be describing a mechanism this build does not have.

mod bind;
mod callbacks;
mod helpers;
mod spawn;

pub use bind::bind;
#[cfg(test)]
pub(super) use helpers::{notifications_enabled, received_notice, received_notification};
pub use spawn::spawn;
#[cfg(test)]
mod tests {
    use super::{received_notice, received_notification};

    #[test]
    fn listener_and_commands_share_the_same_host_and_pairing_desk() {
        let state = super::bind(crate::testing::state());
        let (listener, _) = state.listener.as_ref().expect("binds");
        assert!(std::sync::Arc::ptr_eq(listener, &state.host));
        let incoming = listener.lock().pairing();
        let commands = state.host.lock().pairing();
        assert!(std::sync::Arc::ptr_eq(&incoming, &commands));
    }

    fn received(bundle: Option<&str>) -> synra_core::listener::FinishedTransfer {
        synra_core::listener::FinishedTransfer {
            transfer_id: None,
            path: std::path::PathBuf::from("/inbox/notes.txt"),
            byte_length: 12,
            bundle: bundle.map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id")),
            digest: "sha256-AAAA".to_owned(),
            version: None,
            sync_session_id: None,
            peer: synra_protocol::ids::DeviceId::new(),
        }
    }

    #[test]
    fn a_received_file_is_announced_with_its_name_and_where_it_landed() {
        // ★ The row disappears when the transfer finishes. Without this notice the file simply stops being visible, and
        // the person who needs to find it has no idea which directory it went to.
        let notice = received_notice(&received(None)).expect("a file leaves a notice");

        match notice {
            synra_ipc::event::KernelEvent::Notice { message, params } => {
                assert_eq!(message, "notice.received");
                assert_eq!(params["name"], "notes.txt");
                assert_eq!(params["path"], "/inbox/notes.txt");
            }
            other => panic!("expected a notice, got {other:?}"),
        }
    }

    #[test]
    fn a_plugin_bundle_is_silent_because_installing_it_is_the_news() {
        assert!(received_notice(&received(Some("@acme/chat"))).is_none());
        assert!(
            received_notification(
                &received(Some("@acme/chat")),
                crate::locale::Language::English
            )
            .is_none()
        );
    }

    #[test]
    fn a_received_file_also_asks_for_a_system_notification() {
        // ★ The banner is for the person looking at the screen; this is for the person who is not. A file that arrives
        // silently is one they discover by accident.
        assert_eq!(
            received_notification(&received(None), crate::locale::Language::English),
            Some(("Synra".to_owned(), "Received notes.txt".to_owned()))
        );
        assert_eq!(
            received_notification(&received(None), crate::locale::Language::Chinese),
            Some(("Synra".to_owned(), "已收到 notes.txt".to_owned()))
        );
    }

    #[test]
    fn the_notification_switch_is_on_unless_the_user_turned_it_off() {
        let state = crate::testing::state();
        assert!(
            super::notifications_enabled(&state),
            "the default is to tell someone a file arrived"
        );

        state
            .db()
            .set_config(
                "transfers.notifyOnReceive",
                &serde_json::json!(false),
                1_000,
            )
            .expect("the switch");
        assert!(
            !super::notifications_enabled(&state),
            "the switch was ignored"
        );
    }
}
