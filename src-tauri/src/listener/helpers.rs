use crate::state::AppState;

pub(crate) fn received_name(transfer: &synra_core::listener::FinishedTransfer) -> Option<String> {
    if transfer.bundle.is_some() {
        return None;
    }
    Some(transfer.path.file_name().map_or_else(
        || transfer.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    ))
}

/// The notice a received file leaves, or `None` for a bundle.
///
/// # Why a bundle is silent here
///
/// Because a bundle is not a file the user has to find: it was installed, and the plugins page — and its own event — is
/// where that news belongs. Saying "received @acme/chat" beside "installed @acme/chat" would be two sentences about one
/// arrival.
pub(crate) fn received_notice(
    transfer: &synra_core::listener::FinishedTransfer,
) -> Option<synra_ipc::event::KernelEvent> {
    let name = received_name(transfer)?;
    Some(synra_ipc::event::KernelEvent::Notice {
        message: "notice.received".to_owned(),
        params: serde_json::json!({
            "name": name,
            "path": transfer.path.display().to_string(),
        }),
    })
}

/// The system notification a received file asks for, or `None` for a bundle.
///
/// # Why this is separate from the in-app notice
///
/// Because the window may not be in front. The notice banner is for the person looking at the screen; the system
/// notification is for the person who is not — and a file that arrives silently is one they discover by accident.
pub(crate) fn received_notification(
    transfer: &synra_core::listener::FinishedTransfer,
    language: crate::locale::Language,
) -> Option<(String, String)> {
    let name = received_name(transfer)?;
    Some(("Synra".to_owned(), language.received(&name)))
}

/// Whether the user wants a system notification for a received file.
///
/// Absence is `true`: the design's default is to tell someone a file arrived, and a missing row is "they have not turned
/// it off", not "they turned it on". A row that does not parse is also the default — the kernel wrote it, so a bad value
/// is a bug, and silently muting the notification would hide it.
pub(crate) fn notifications_enabled(state: &AppState) -> bool {
    state
        .db()
        .config()
        .ok()
        .and_then(|rows| {
            rows.into_iter()
                .find(|row| row.key == "transfers.notifyOnReceive")
        })
        .and_then(|row| serde_json::from_str::<bool>(&row.value).ok())
        .unwrap_or(true)
}

/// The intent a peer's invocation raised, looked up by the pair that identifies it.
///
/// # Why the lookup rather than a returned identifier
///
/// Because `accept_remote` answers with the **reply** it built, not with the intent it raised: the caller's job is to put
/// the answer on the link. The board is the authority on which intent that was, and `recall` is the same lookup the
/// idempotency rule uses — origin plus request, never one of them alone.
pub(crate) fn asked_intent(
    state: &AppState,
    envelope: &synra_protocol::envelope::Envelope,
) -> Option<synra_protocol::ids::IntentId> {
    let board = state.board.lock();
    board
        .recall(envelope.from, envelope.request_id)
        .map(|intent| intent.id)
}

/// Tells the interface, and writes the row, that a remote event changed an intent.
///
/// # Why both, and in this order
///
/// Because they answer different losses. The announcement is what makes the change visible now — a remote action that
/// ran and never appeared on the home page looks like it did not run. The row is what makes it survive the next start;
/// the write is last because it is the slow half and because a failed write must not suppress the event.
pub(crate) fn record_and_announce(state: &AppState, intent: synra_protocol::ids::IntentId) {
    crate::commands::intent::announce(state, intent);
    crate::commands::intent::checkpoint(state, intent);
}
