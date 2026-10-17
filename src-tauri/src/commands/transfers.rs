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
    by_id: Mutex<HashMap<TransferId, Entry>>,
}

/// One transfer: what the interface shows, and what this shell owns on disk.
#[derive(Debug, Clone)]
struct Entry {
    /// What the panel draws.
    snapshot: TransferSnapshot,
    /// A file this shell created for the transfer, to be removed when the transfer ends.
    ///
    /// Carried here rather than recomputed from the snapshot because the path is not derivable: a sent file belongs to the user
    /// and must **not** be deleted, while a sent message was written by this shell and must be. The two look identical from the
    /// snapshot alone.
    temp: Option<std::path::PathBuf>,
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
    ///
    /// The temporary file is **kept**: this is called on every block, and a version that cleared it would forget what to delete
    /// on the second report — which is the one moment a cancel is likely to arrive.
    pub fn record(&self, snapshot: TransferSnapshot) {
        let mut registry = self.by_id.lock();
        let temp = registry.get(&snapshot.transfer_id).and_then(|entry| entry.temp.clone());
        registry.insert(snapshot.transfer_id, Entry { snapshot, temp });
    }

    /// Records a snapshot and the file this shell created for it.
    pub fn record_with_temp(&self, snapshot: TransferSnapshot, temp: std::path::PathBuf) {
        self.by_id.lock().insert(snapshot.transfer_id, Entry { snapshot, temp: Some(temp) });
    }

    /// Drops a transfer, answering the file the caller should delete.
    ///
    /// Returning the path rather than deleting it here keeps this type free of the filesystem: the registry's job is to know
    /// what is in flight, and a caller that cancels is the one that knows whether the transfer was a message of its own.
    pub fn forget(&self, transfer_id: &TransferId) -> Option<std::path::PathBuf> {
        self.by_id.lock().remove(transfer_id).and_then(|entry| entry.temp)
    }

    /// Every transfer, ordered by identifier.
    ///
    /// Ordered by identifier rather than by start time because the list is read repeatedly: sorting by arrival would let two
    /// transfers swap places between two reads, which the interface would draw as two rows moving for no reason.
    #[must_use]
    pub fn list(&self) -> Vec<TransferSnapshot> {
        let mut transfers: Vec<TransferSnapshot> = self.by_id.lock().values().map(|entry| entry.snapshot.clone()).collect();
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
    // ★ The file this shell created for the transfer goes with it. A cancel that only removed the row would leave a message
    // on disk that nobody will ever send and nothing will ever delete.
    if let Some(path) = state.transfers.forget(&id) {
        let _ = std::fs::remove_file(path);
    }
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

/// Sends a file to a paired device, and answers the transfer it started.
///
/// # Where the address comes from
///
/// `paired_devices`, not `devices_seen`: a transfer's target must already be paired, and that table is the one that records
/// where a device is. `None` is not bad data — it means this device has never been dialled — so it becomes
/// [`ErrorCode::Unsupported`] with a reason a person can act on.
///
/// # Why the row is recorded before the dial
///
/// So a send that fails to connect leaves a row saying why instead of nothing at all. The row is removed on every exit path and
/// `TransferFinished` is announced on each: a transfer that stayed in the registry after failing would look like one still
/// running.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] for an unknown or address-less device, [`ErrorCode::InvalidArgument`] for a file that cannot be
/// planned, and [`ErrorCode::TransferFailed`] for a dial or a block the link refused.
pub fn transfer_send_file(
    state: &crate::state::AppState,
    request: &synra_ipc::dto::SendFileRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let address = address_of(state, request.to)?;

    let sender = synra_core::transfer::TransferSender::open(
        std::path::Path::new(&request.path),
        synra_core::transfer::DEFAULT_CHUNK_SIZE,
    )
    .map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": request.path, "reason": error.to_string()}))
    })?;
    let planned = sender.request();

    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(&planned, (request.to, &request.to.to_string()), "queued", now_ms);
    // The row names the file a person chose, not the path the interface happened to pass.
    row.file_name = std::path::Path::new(&request.path)
        .file_name()
        .map_or_else(|| request.path.clone(), |name| name.to_string_lossy().into_owned());

    send_over(state, request.to, address, sender, row)
}

/// Dials, moves every block, and reports each one — the part two commands share.
///
/// # Why this is a function
///
/// Because `transfer_send_file` and `transfer_send_text` differ only in how they obtain a [`TransferSender`] and what the row
/// is named: everything after "there is a sender and an address" is the same dial, the same loop and the same three exit
/// paths. Two copies would be two places for the cleanup to be forgotten, and the cleanup is what keeps a failed transfer
/// from looking like a running one.
///
/// # Errors
///
/// [`ErrorCode::TransferFailed`] for a refused dial or a refused block, after removing the row either way.
pub fn send_over(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    address: std::net::SocketAddr,
    mut sender: synra_core::transfer::TransferSender,
    mut row: TransferSnapshot,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let transfer_id = row.transfer_id;
    let identity = state.identity.lock().clone();
    let now_ms = unix_millis();
    announce_transfer(state, row.clone());

    let finish = |state: &crate::state::AppState| {
        state.transfers.forget(&transfer_id);
        state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
    };

    let dialled = match synra_core::host::dial_peer(
        &local_peer(&identity),
        synra_core::peer::Flags::production(),
        address,
        None,
        now_ms,
    ) {
        Ok(dialled) => dialled,
        Err(error) => {
            finish(state);
            return Err(IpcError::unknown(ErrorCode::TransferFailed)
                .with_params(serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()})));
        }
    };
    let mut link = dialled.into_link(identity.device_id, synra_core::peer::Flags::production());

    row.state = "active".to_owned();
    announce_transfer(state, row.clone());

    let outcome = synra_core::transfer::send_blocks(
        &mut sender,
        |index, data| link.send_chunk(index, data),
        |done, _total| {
            row.blocks_done = done;
            announce_transfer(state, row.clone());
        },
    );
    finish(state);

    outcome.map(|()| transfer_id).map_err(|error| {
        IpcError::unknown(ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()}))
    })
}

/// The wall clock, in milliseconds, as the counts this module reports are measured.
fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_u64, |elapsed| elapsed.as_millis() as u64)
}

/// Where a paired device can be reached, or a refusal that says which kind of missing this is.
///
/// # Why the two failures are different answers
///
/// [`ErrorCode::Unsupported`] covers both "never paired" and "paired, but this side has never seen an address", and it says
/// which in the parameters — the first needs the user to pair, the second needs the user to connect once. A single "cannot
/// send" would leave them guessing.
///
/// # Errors
///
/// [`ErrorCode::Internal`] when the database cannot be read, and [`ErrorCode::Unsupported`] as above.
pub fn address_of(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
) -> Result<std::net::SocketAddr, IpcError> {
    use synra_protocol::ErrorCode;

    let address = state
        .db()
        .paired_devices()
        .map_err(|error| {
            IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({"detail": error.to_string()}))
        })?
        .into_iter()
        .find(|device| device.device_id == to.to_string())
        .and_then(|device| device.address);

    let Some(address) = address else {
        return Err(IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"deviceId": to.to_string(), "reason": "it has no address"})));
    };
    address.parse::<std::net::SocketAddr>().map_err(|error| {
        IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"address": address, "reason": error.to_string()}))
    })
}

/// Sends text as a file, and answers the transfer it started.
///
/// # Why text is a file
///
/// Because the transport has no notion of one: [`TransferSender`] opens a path, and the wire's metadata has `Attachment`
/// and `PluginBundle` and nothing else. The alternative — a new kind for text — would be a protocol change to save writing one
/// file and deleting it again.
///
/// # What this leaves behind if it is cancelled
///
/// The file. `transfer_cancel` removes the registry row and cannot reach the path, which is a known gap rather than an
/// oversight: the fix is for the registry to carry the path so a worker can delete it, and that waits on the worker.
///
/// # Errors
///
/// As [`transfer_send_file`], plus [`ErrorCode::Internal`] when the temporary file cannot be written.
pub fn transfer_send_text(
    state: &crate::state::AppState,
    request: &synra_ipc::dto::SendTextRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let address = address_of(state, request.to)?;

    // One file per transfer, named for a person rather than for a uuid: `open` derives the name the peer sees from the path.
    //
    // ★ Under the **cache** directory rather than the system one: the system temporary directory is shared with every other
    // process on the machine, and this file holds something a person typed. A per-user directory is the difference between a
    // file that is deleted and a file that was readable by anyone while it existed.
    let outbox = state.paths.cache_dir().join("outbox");
    std::fs::create_dir_all(&outbox).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"path": outbox.display().to_string(), "reason": error.to_string()}))
    })?;
    let path = outbox.join(format!("message-{}.txt", unix_millis()));
    std::fs::write(&path, request.text.as_bytes()).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()}))
    })?;

    let sender = synra_core::transfer::TransferSender::open(&path, synra_core::transfer::DEFAULT_CHUNK_SIZE)
        .inspect_err(|_| {
            // ★ Removed here as well as after the send: a sender that refused the file must not leave it on disk for the next
            // run to find, and the delete below is only reached when this call succeeded.
            let _ = std::fs::remove_file(&path);
        })
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()}))
        })?;

    let planned = sender.request();
    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(&planned, (request.to, &request.to.to_string()), "queued", now_ms);
    row.kind = "attachment".to_owned();

    // Registered with its file before the send starts, so a cancel that arrives mid-send has something to delete.
    state.transfers.record_with_temp(row.clone(), path.clone());
    let outcome = send_over(state, request.to, address, sender, row);
    // Every exit path, including the one where the link refused every block.
    let _ = std::fs::remove_file(&path);
    outcome
}

#[cfg(test)]
mod tests;
