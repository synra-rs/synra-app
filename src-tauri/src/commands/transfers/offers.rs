use super::*;

/// The row a receiver reports while a transfer arrives.
///
/// # Why the receiver's row is built here rather than in the listener
///
/// Because this module owns the shape the panel draws, and the outgoing path builds its row here too. Two constructors
/// for one DTO would be two places for a field to be forgotten — and a forgotten `direction` would show an arriving file
/// as one this device sent.
#[must_use]
pub fn received_snapshot(
    event: &synra_core::listener::InboundProgressEvent,
    peer_name: &str,
    started_at_ms: i64,
) -> TransferSnapshot {
    let progress = &event.progress;
    TransferSnapshot {
        transfer_id: progress.transfer_id,
        peer_device: event.peer,
        peer_name: peer_name.to_owned(),
        direction: "receive".to_owned(),
        kind: if progress.bundle.is_some() {
            "plugin-bundle".to_owned()
        } else {
            "attachment".to_owned()
        },
        state: "active".to_owned(),
        file_name: progress.file_name.clone(),
        received_bytes: progress.received_bytes,
        total_bytes: progress.total_bytes,
        blocks_done: progress.blocks_done,
        blocks_total: progress.blocks_total,
        abort_reason: None,
        started_at_ms,
    }
}

/// Closes the row of a transfer that finished on this side, and tells the interface.
///
/// # Why this is one function
///
/// Because the two halves must not be separated: a row left in the registry is a transfer the panel shows as still
/// arriving, and a `TransferFinished` without the removal leaves a row that no later event can clear. The outgoing path
/// has the same pair in [`finish_transfer`], and this is the receiving half of it.
/// The row a transfer prompt is built from, before any byte has arrived.
///
/// `state` is `offered`, which is the word the interface uses for "this is a question, not a transfer". The counts are
/// real: the proposal says how many bytes and how it will be split, so the prompt can show a size without reading
/// anything.
#[must_use]
pub fn offered_snapshot(
    offer: &synra_core::inbound::Offer<'_>,
    peer_name: &str,
    started_at_ms: i64,
) -> TransferSnapshot {
    let request = offer.request;
    let blocks_total = if request.chunk_size == 0 {
        0
    } else {
        u32::try_from(request.byte_length.div_ceil(u64::from(request.chunk_size)))
            .unwrap_or(u32::MAX)
    };
    TransferSnapshot {
        transfer_id: request.transfer_id,
        peer_device: offer.peer,
        peer_name: peer_name.to_owned(),
        direction: "receive".to_owned(),
        kind: match request.kind {
            synra_protocol::session::TransferKind::PluginBundle => "plugin-bundle",
            _ => "attachment",
        }
        .to_owned(),
        state: "offered".to_owned(),
        file_name: offer.file_name.to_owned(),
        received_bytes: 0,
        total_bytes: request.byte_length,
        blocks_done: 0,
        blocks_total,
        abort_reason: None,
        started_at_ms,
    }
}

/// Removes a prompt's row, whichever way it was answered.
///
/// The row was never a transfer in flight, so there is no worker to forget it and no temporary file to delete: the panel
/// has to be told, and the registry has to stop listing it.
pub fn withdraw_offer(state: &crate::state::AppState, transfer_id: TransferId) {
    state.transfers.forget(&transfer_id);
    state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
}

/// Keeps a published offer alive until its question retires, before its registry slot can be reused.
pub struct OfferNotice<'a> {
    state: &'a crate::state::AppState,
    transfer_id: TransferId,
}

impl Drop for OfferNotice<'_> {
    fn drop(&mut self) {
        withdraw_offer(self.state, self.transfer_id);
    }
}

/// Publishes an offer whose guard withdraws only this admitted question.
#[must_use]
pub fn announce_offer(
    state: &crate::state::AppState,
    snapshot: TransferSnapshot,
) -> OfferNotice<'_> {
    let transfer_id = snapshot.transfer_id;
    announce_transfer(state, snapshot);
    OfferNotice { state, transfer_id }
}

/// Answers the prompt for an incoming transfer.
///
/// # The `remember` flag
///
/// A valid question is settled once, only after a requested preference is persisted. Storage failure leaves the question
/// unanswered for explicit retry. The database is acquired before the per-question lock; the registry lock is never held
/// while accessing storage. A refusal never persists a preference.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] when the identifier does not parse, and [`ErrorCode::TransferFailed`] when no prompt is
/// waiting for it — an id that expired while the user was deciding, or one the interface repeated.
pub fn transfer_respond(
    state: &crate::state::AppState,
    transfer_id: &str,
    accept: bool,
    remember: Option<bool>,
) -> Result<(), IpcError> {
    let id = transfer_id.parse::<TransferId>().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"transferId": transfer_id, "detail": error.to_string()}),
        )
    })?;
    let mut changed = false;
    let answered = {
        let db = accept.then(|| state.db());
        state.transfer_desk.respond(&id.to_string(), accept, |peer| {
            if let Some(db) = db.as_deref() {
                if !db.is_paired_with(peer).map_err(crate::state::internal)? {
                    return Err(IpcError::unknown(synra_protocol::ErrorCode::TransferFailed)
                        .with_params(serde_json::json!({"transferId": transfer_id, "reason": "the sender is no longer paired or is blocked"})));
                }
                if remember.unwrap_or(false) {
                    changed = remember_peer(db, peer)?;
                }
            }
            Ok(())
        })?
    };
    if changed {
        crate::commands::config::announce_config(state);
    }
    if answered {
        return Ok(());
    }
    Err(
        IpcError::unknown(synra_protocol::ErrorCode::TransferFailed).with_params(
            serde_json::json!({
                "transferId": transfer_id,
                "reason": "it is not waiting for an answer",
            }),
        ),
    )
}

/// The configuration key holding the devices this one accepts without asking.
pub(crate) const ACCEPT_FROM_KEY: &str = "transfers.acceptFrom";

/// Validates the security preference at both the settings and stored-data boundaries.
pub(crate) fn validate_accepted_senders(
    value: &serde_json::Value,
) -> Result<Vec<String>, IpcError> {
    let accepted: Vec<String> = serde_json::from_value(value.clone()).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"key": ACCEPT_FROM_KEY, "reason": error.to_string()}))
    })?;
    if accepted.len() > 256
        || accepted
            .iter()
            .any(|peer| peer.parse::<synra_protocol::DeviceId>().is_err())
    {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"key": ACCEPT_FROM_KEY, "reason": "expected at most 256 device IDs"})));
    }
    Ok(accepted)
}

/// The devices this one accepts without asking.
fn accepted_senders(db: &synra_store::SynraDb) -> Result<Vec<String>, IpcError> {
    let rows = db.config().map_err(crate::state::internal)?;
    let Some(row) = rows.into_iter().find(|row| row.key == ACCEPT_FROM_KEY) else {
        return Ok(Vec::new());
    };
    let value = serde_json::from_str(&row.value).map_err(crate::state::internal)?;
    validate_accepted_senders(&value)
}

/// Whether the user has chosen to accept everything from this device.
///
/// A sender that is not listed is asked about, which is the default and the only state a fresh install has.
#[must_use]
pub fn auto_accept_peer(state: &crate::state::AppState, peer: &str) -> bool {
    let db = state.db();
    db.is_paired_with(peer).unwrap_or(false)
        && accepted_senders(&db)
            .unwrap_or_default()
            .iter()
            .any(|accepted| accepted == peer)
}

/// Forgets a device the user chose to accept without asking.
///
/// Called when the device itself is forgotten: a trusted sender that is no longer paired is a preference about a device
/// this one no longer knows, and keeping it would silently re-trust the same identifier if it ever came back.
pub(crate) fn forget_sender(db: &synra_store::SynraDb, peer: &str) -> Result<bool, IpcError> {
    let mut accepted = accepted_senders(db)?;
    let before = accepted.len();
    accepted.retain(|known| known != peer);
    if accepted.len() == before {
        return Ok(false);
    }
    let now = synra_store::sqlite::now_ms();
    if accepted.is_empty() {
        // Unset rather than store an empty array: an empty preference is no preference, and a row saying "none" is a
        // setting a screen would have to interpret.
        db.unset_config(ACCEPT_FROM_KEY)
            .map_err(crate::state::internal)?;
    } else {
        db.set_config(ACCEPT_FROM_KEY, &serde_json::json!(accepted), now)
            .map_err(crate::state::internal)?;
    }
    Ok(true)
}

/// Remembers a device the user chose to accept without asking.
///
/// Stored under one configuration key as a JSON array of device ids, so the settings screen can show (and one day remove)
/// it without a table of its own. The caller owns the database guard across this read/modify/write so concurrent answers
/// cannot discard each other's preferences. A write failure is returned before the question is settled.
pub(super) fn remember_peer(db: &synra_store::SynraDb, peer: &str) -> Result<bool, IpcError> {
    let mut accepted = accepted_senders(db)?;
    if accepted.iter().any(|known| known == peer) {
        return Ok(false);
    }
    accepted.push(peer.to_owned());
    let value = serde_json::json!(accepted);
    validate_accepted_senders(&value)?;
    db.set_config(ACCEPT_FROM_KEY, &value, synra_store::sqlite::now_ms())
        .map_err(crate::state::internal)?;
    Ok(true)
}
