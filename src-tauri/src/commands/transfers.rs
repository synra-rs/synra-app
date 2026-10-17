//! The transfers this shell is running, and the command that lists them.
//!
//! # Why a registry rather than a read from the link
//!
//! Because a transfer outlives the frame that started it: it is a sequence of chunks, and the interface has to be able to ask
//! "what is in flight" between two of them. The registry is written by the worker that moves the chunks and read by
//! `transfer_list`; the two never hold the lock for longer than a lookup.
//!
//! # Why the snapshot is stored rather than derived
//!
//! The worker already has the numbers — how far along, how many blocks — and deriving them again here would be a second
//! implementation of the progress arithmetic. The snapshot is what the worker last reported, and this module's job is to
//! keep that honest rather than to improve on it.

use std::collections::HashMap;

use parking_lot::Mutex;
use synra_ipc::dto::TransferSnapshot;
use synra_ipc::IpcError;
use synra_protocol::ids::TransferId;

/// The transfers in flight, by identifier.
#[derive(Debug, Default)]
pub struct Transfers {
    /// Keyed by the transfer, not ordered: `list` sorts, so the worker never has to.
    by_id: Mutex<HashMap<TransferId, TransferSnapshot>>,
}

impl Transfers {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what the worker just reported, replacing the previous snapshot of the same transfer.
    ///
    /// Replacement rather than merge: a snapshot is a complete answer, and merging two would let a field the worker stopped
    /// reporting keep its old value for ever — a progress bar that freezes while the transfer moves.
    pub fn record(&self, snapshot: TransferSnapshot) {
        self.by_id.lock().insert(snapshot.transfer_id, snapshot);
    }

    /// Drops a transfer, which is what "finished" means for this list.
    pub fn forget(&self, transfer_id: &TransferId) {
        self.by_id.lock().remove(transfer_id);
    }

    /// Every transfer, ordered by identifier.
    ///
    /// Ordered by identifier rather than by start time because the list is read repeatedly: sorting by arrival would let two
    /// transfers swap places between two reads, which the interface would draw as two rows moving for no reason.
    #[must_use]
    pub fn list(&self) -> Vec<TransferSnapshot> {
        let mut transfers: Vec<TransferSnapshot> = self.by_id.lock().values().cloned().collect();
        transfers.sort_by_key(|transfer| transfer.transfer_id);
        transfers
    }

    /// How many are in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.lock().len()
    }

    /// Whether nothing is in flight.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.lock().is_empty()
    }
}

/// Reports one transfer's state: records it, and tells the interface.
///
/// # Why the two happen together
///
/// Because a snapshot that is recorded but not announced is a panel that is right the next time it opens and wrong until
/// then, and one that is announced but not recorded is a `transfer_list` that disagrees with what the panel shows. The worker
/// calls this on every change; nothing else should have to remember the pair.
///
/// # What it does not decide
///
/// Whether the transfer is over. A worker that knows a transfer finished calls [`Transfers::forget`] and publishes
/// `TransferFinished` itself — this function cannot tell from the snapshot alone whether "finished" means done or failed, and
/// guessing that would put a transfer's fate in the wrong place.
pub fn announce_transfer(state: &crate::state::AppState, snapshot: TransferSnapshot) {
    let transfer_id = snapshot.transfer_id;
    state.transfers.record(snapshot.clone());
    state.publish(synra_ipc::event::KernelEvent::TransferUpdated(Box::new(snapshot)));
    // The identifier is kept above so the event and the record cannot name different transfers, which is the failure a
    // second read of `snapshot.transfer_id` after the move would allow.
    debug_assert_eq!(
        state.transfers.list().iter().filter(|t| t.transfer_id == transfer_id).count(),
        1,
        "one record per transfer"
    );
}

/// The row a worker reports for a sender it just started.
///
/// # Why the peer is an argument
///
/// Because [`TransferRequest`] does not carry one: who the other device is, only the link knows. A version that read the peer
/// from the request would have to invent it, and a progress row naming the wrong device is worse than no row.
///
/// # Why the state is an argument too
///
/// Because "queued" and "active" are the worker's judgement — it knows whether a link accepted the first chunk — and a
/// snapshot this module guessed would be describing a transfer it is not running.
#[must_use]
pub fn snapshot_from(
    request: &synra_protocol::session::TransferRequest,
    peer: (synra_protocol::ids::DeviceId, &str),
    state: &str,
    started_at_ms: i64,
) -> TransferSnapshot {
    use synra_protocol::session::TransferMeta;

    // The two kinds carry different things a person reading the panel needs: which plugin, or which file. Everything else
    // about them is the same row, so the difference stays here rather than in a trait.
    let (kind, file_name) = match &request.meta {
        TransferMeta::PluginBundle { plugin_id, version } => {
            ("plugin-bundle".to_owned(), format!("{plugin_id} {version}"))
        }
        TransferMeta::Attachment { file_name, .. } => ("attachment".to_owned(), file_name.clone()),
        // TransferMeta is #[non_exhaustive]: a kind this build does not know is still a row, just a nameless one.
        _ => ("unknown".to_owned(), String::new()),
    };
    let blocks_total = u32::try_from(request.byte_length.div_ceil(u64::from(request.chunk_size))).unwrap_or(u32::MAX);

    TransferSnapshot {
        transfer_id: request.transfer_id,
        peer_device: peer.0,
        peer_name: peer.1.to_owned(),
        direction: "send".to_owned(),
        kind,
        state: state.to_owned(),
        file_name,
        received_bytes: 0,
        total_bytes: request.byte_length,
        blocks_done: 0,
        blocks_total,
        abort_reason: None,
        started_at_ms,
    }
}

/// The transfers in flight, for the interface's progress panel.
///
/// The list is the registry's own order (by identifier), not the arrival order: the panel redraws on every event, and rows
/// that swap places between two redraws look like rows that are moving for no reason.
#[must_use]
pub fn transfer_list(state: &crate::state::AppState) -> Vec<TransferSnapshot> {
    state.transfers.list()
}

/// Abandons a transfer, and answers whether there was one.
///
/// # Why this refuses an unknown identifier
///
/// Because a cancel naming a transfer that is not in flight nearly always means the interface's picture is out of date, and
/// answering "done" would leave it believing it stopped something. The refusal names the identifier so a stale panel can
/// correct itself by re-reading.
///
/// # What it does not do yet
///
/// It does not tell the peer. A cancellation is a frame, and the link that would carry it is what the worker is for; today
/// this stops the shell's own record, which is the part the progress panel reads.
pub fn transfer_cancel(state: &crate::state::AppState, transfer_id: &str) -> Result<(), IpcError> {
    let id = transfer_id.parse::<TransferId>().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"transferId": transfer_id, "detail": error.to_string()}))
    })?;

    if state.transfers.list().iter().all(|transfer| transfer.transfer_id != id) {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"transferId": transfer_id, "reason": "it is not in flight"})));
    }
    state.transfers.forget(&id);
    // ★ The panel learns without asking. `TransferFinished` removes the row in the intents store, and without this a
    // cancelled transfer would sit in the progress list until the page was reopened — the same silent staleness every other
    // command here was fixed for.
    state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id: id });
    Ok(())
}

/// What this shell says about itself when it dials a peer.
///
/// # Why the platform is mapped rather than passed through
///
/// Because the identity stores the platform as a string (what the installer recorded) while the wire wants `PlatformKind`,
/// and the two must not drift: a peer reading "windows" where it expected an enum would decline the handshake, and the shell
/// would look like a broken build rather than a mistyped one.
///
/// # Why the display name comes from the identity
///
/// Because that is the name the user set and the one the discovery responder already announces. A second source would let
/// one device appear under two names on the same network.
#[must_use]
pub fn local_peer(identity: &synra_core::identity::Identity) -> synra_core::peer::LocalPeer {
    use synra_protocol::session::PlatformKind;

    let platform = match identity.platform.to_ascii_lowercase().as_str() {
        "android" => PlatformKind::Android,
        "ios" | "macos" | "mac" => PlatformKind::Ios,
        _ => PlatformKind::Desktop,
    };
    synra_core::peer::LocalPeer::new(
        identity.device_id,
        identity.display_name.clone(),
        platform,
        env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod tests;
