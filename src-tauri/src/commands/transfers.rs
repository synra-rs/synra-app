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
    /// The token that stops each worker, when a transfer has one.
    ///
    /// Beside the snapshots rather than inside them: a token is not part of what a panel draws, and a `TransferSnapshot`
    /// that carried one would have to be `Clone` over a handle or lose it in the clone.
    cancels: Mutex<HashMap<TransferId, synra_core::supervisor::Cancellation>>,
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

    /// Remembers the token that stops the worker for this transfer.
    pub fn track_cancel(
        &self,
        transfer_id: TransferId,
        cancel: synra_core::supervisor::Cancellation,
    ) {
        self.cancels.lock().insert(transfer_id, cancel);
    }

    /// Takes the token out and asks it to stop.
    ///
    /// Answers whether a worker was there to ask. A token is removed on the way out: a cancel arriving after the worker
    /// finished must not leave one behind for a later transfer that reuses the identifier (identifiers are v4 and do not
    /// repeat, but a map that only grows is a leak either way).
    pub fn stop(&self, transfer_id: &TransferId) -> bool {
        match self.cancels.lock().remove(transfer_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Forgets a token whose worker has finished on its own.
    pub fn untrack(&self, transfer_id: &TransferId) {
        self.cancels.lock().remove(transfer_id);
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
/// # What it does for a transfer arriving
///
/// Nothing immediate: the listener's connection thread owns that link, so the identifier is left in
/// [`crate::state::AppState::inbound_cancels`] and consumed between messages. The loop sends the abort, removes the
/// partial file and reports an `aborted` progress event, which is what withdraws the row.
pub fn transfer_cancel(state: &crate::state::AppState, transfer_id: &str) -> Result<(), IpcError> {
    let id = transfer_id.parse::<TransferId>().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"transferId": transfer_id, "detail": error.to_string()}))
    })?;

    if state.transfers.list().iter().all(|transfer| transfer.transfer_id != id) {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"transferId": transfer_id, "reason": "it is not in flight"})));
    }
    // ★ Two directions, two mechanisms. An outgoing transfer has a worker holding a cancellation token; an incoming one
    // has a connection thread that owns the link, so the command can only leave the identifier where that loop will look.
    if state.transfers.stop(&id) {
        // ★ The file this shell created for the transfer goes with it. A cancel that only removed the row would leave a
        // message on disk that nobody will ever send and nothing will ever delete.
        if let Some(path) = state.transfers.forget(&id) {
            let _ = std::fs::remove_file(path);
        }
        // ★ The panel learns without asking. `TransferFinished` removes the row in the intents store, and without this a
        // cancelled transfer would sit in the progress list until the page was reopened — the same silent staleness every
        // other command here was fixed for.
        state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id: id });
    }
    else {
        // The row stays: the listener has not seen the request yet, and removing it here would make the interface forget a
        // transfer that is still running. It goes when the listener's abort reaches the progress callback.
        state.inbound_cancels.lock().insert(id.to_string());
    }
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
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_over_with(state, to, address, sender, row, None)
}

/// [`send_over`], with a token a cancel can stop.
pub fn send_over_with(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    address: std::net::SocketAddr,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let started = std::time::Instant::now();
    let kind = row.kind.clone();
    let total_bytes = row.total_bytes;
    let transfer_id = row.transfer_id;
    let identity = state.identity.lock().clone();
    let now_ms = unix_millis();

    let dialled = match synra_core::host::dial_peer(
        &local_peer(&identity),
        synra_core::peer::Flags::production(),
        address,
        None,
        now_ms,
    ) {
        Ok(dialled) => dialled,
        Err(error) => {
            // ★ A dial that never connected is still an attempt, and the one a user most wants counted: it is the failure
            // that happens before any block moves. `send_ready_since` records the other exits.
            record_send(state, &kind, total_bytes, started.elapsed(), SendOutcome::Failed);
            finish_transfer(state, transfer_id);
            return Err(IpcError::unknown(ErrorCode::TransferFailed)
                .with_params(serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()})));
        }
    };
    let mut link = dialled
        .into_link(identity.device_id, synra_core::peer::Flags::production())
        // ★ The byte counters live on the link; without this the shell would own a recorder nothing on its dials wrote to.
        .with_metrics(std::sync::Arc::clone(&state.metrics));
    send_ready_since(state, to, &mut link, sender, row, cancel, started)
}

/// Moves the blocks over a link that is already connected, and reports each one.
///
/// # Why the link is a parameter
///
/// Because this is the half that needs testing, and a test that must dial cannot be run without a peer. Everything here is
/// driven by what arrives on the link, so a memory link exercises it exactly as a socket does — the dial above is the only
/// part that has to be real.
pub fn send_ready(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_ready_with(state, to, link, sender, row, None)
}

/// [`send_ready`], with a token a cancel can stop between blocks.
///
/// # Why the token is checked here rather than by the transfer loop
///
/// Because the loop is in the kernel and has no notion of a user. The check sits in the `send` closure, which is the one
/// place the loop asks before writing a block; a cancelled token turns into a refusal, the loop stops, and the peer is
/// told with an abort frame rather than left waiting for blocks that will never come.
pub fn send_ready_with(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_ready_since(state, to, link, sender, row, cancel, std::time::Instant::now())
}

/// The body both entries share.
///
/// `started` is when the attempt began — before the dial when there is one, so `transfer_duration_ms` covers the whole
/// attempt rather than only the block loop. A caller that already has a link passes `Instant::now()`.
fn send_ready_since(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    mut sender: synra_core::transfer::TransferSender,
    mut row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
    started: std::time::Instant,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let transfer_id = row.transfer_id;
    announce_transfer(state, row.clone());

    row.state = "active".to_owned();
    announce_transfer(state, row.clone());

    let cancelled = || cancel.is_some_and(|token| token.is_cancelled());
    let outcome = synra_core::transfer::send_blocks(
        &mut sender,
        |index, data| {
            if cancelled() {
                return Err(SendStop::Cancelled);
            }
            link.send_chunk(index, data).map_err(SendStop::Link)
        },
        |done, _total| {
            if !cancelled() {
                row.blocks_done = done;
                announce_transfer(state, row.clone());
            }
        },
    );
    let ended = match &outcome {
        Ok(()) => SendOutcome::Delivered,
        Err(synra_core::transfer::SendBlocksError::Send(SendStop::Cancelled)) => SendOutcome::Cancelled,
        Err(_) => SendOutcome::Failed,
    };
    record_send(state, &row.kind, row.total_bytes, started.elapsed(), ended);

    if ended == SendOutcome::Cancelled {
        // ★ The peer is told, not merely dropped. A sender that closes without an abort sees its blocks stop arriving and
        // waits out its own transfer window; an abort ends the round on both sides.
        let _ = synra_core::abort(
            link,
            transfer_id,
            synra_protocol::session::TransferAbortReason::UserCancelled,
            synra_protocol::ids::TraceId::new(),
            unix_millis() as i64,
        );
    }
    finish_transfer(state, transfer_id);

    outcome.map(|()| transfer_id).map_err(|error| {
        IpcError::unknown(ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()}))
    })
}

/// Why an outbound transfer ended, as the metric label spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    /// Every block reached the link.
    Delivered,
    /// The user stopped it before the loop finished.
    Cancelled,
    /// The link refused a block, or the loop failed.
    Failed,
}

impl SendOutcome {
    /// The label `transfers_total{outcome}` carries.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Records one outbound attempt: its result, how long it took, and — only when it delivered — how large it was.
///
/// # Why `transfer_bytes` only on delivery
///
/// The histogram answers "how large are the files this device sends". A cancelled or failed transfer never moved the
/// whole file, so recording the planned size would describe bytes that never went. The attempt itself is in
/// `transfers_total{kind, outcome}` and the time it cost is in `transfer_duration_ms`.
fn record_send(
    state: &crate::state::AppState,
    kind: &str,
    total_bytes: u64,
    elapsed: std::time::Duration,
    outcome: SendOutcome,
) {
    record_attempt(
        state,
        kind,
        total_bytes,
        Some(elapsed),
        outcome.as_str(),
        outcome == SendOutcome::Delivered,
    );
}

/// Records one inbound attempt, as the receiving half saw it.
///
/// # Why an aborted transfer records no size
///
/// For the same reason a cancelled send does not: the size histogram describes files that arrived. The attempt is still
/// counted (`outcome=aborted`), because "someone sent and it did not land" is the fact the user is asking about.
///
/// # Why the elapsed time is optional
///
/// The listener learns a transfer started from its first progress event. An abort that arrives before that event has no
/// start to subtract, and inventing a zero would put a transfer that never moved into the latency distribution.
///
/// ★ The only caller is the listener, which is compiled only into the shell build (`tauri-shell`). The no-default build
/// still has this function because its unit test runs there, so it is dead code **in that build only** — allowed here
/// rather than hidden behind a `cfg(test)` that would make the function untestable from the shell's own module.
#[cfg_attr(not(feature = "tauri-shell"), allow(dead_code))]
pub(crate) fn record_received(
    state: &crate::state::AppState,
    kind: &str,
    byte_length: u64,
    elapsed: Option<std::time::Duration>,
    aborted: bool,
) {
    record_attempt(
        state,
        kind,
        byte_length,
        elapsed,
        if aborted { "aborted" } else { "received" },
        !aborted,
    );
}

/// The shape both directions share.
///
/// # Why the names are the daemon's
///
/// The daemon writes `transfer_bytes{kind}` and `transfer_duration_ms{kind}`, so the same transfer must not be called
/// something else by the other host. `transfers_total` adds `outcome` because the daemon deliberately left it out — its
/// exits are not funnelled, and a counter written on the success path alone would answer the wrong question.
#[allow(clippy::cast_precision_loss)]
fn record_attempt(
    state: &crate::state::AppState,
    kind: &str,
    byte_length: u64,
    elapsed: Option<std::time::Duration>,
    outcome: &str,
    record_bytes: bool,
) {
    state.metrics.counter_inc(&synra_observe::metrics::SeriesKey::new(
        "transfers_total",
        [
            ("kind".to_owned(), kind.to_owned()),
            ("outcome".to_owned(), outcome.to_owned()),
        ],
    ));
    if let Some(elapsed) = elapsed {
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "transfer_duration_ms",
                [("kind".to_owned(), kind.to_owned())],
            ),
            elapsed.as_secs_f64() * 1_000.0,
        );
    }
    if record_bytes {
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new("transfer_bytes", [("kind".to_owned(), kind.to_owned())]),
            byte_length as f64,
        );
        // ★ Throughput only for a transfer that moved its bytes, and only when there is a duration to divide by. The
        // window is the whole attempt (for a send, including the dial), so this is end-to-end throughput rather than the
        // link's line rate — which is the number a user comparing "how long did this take" is asking about.
        if let Some(elapsed) = elapsed {
            let seconds = elapsed.as_secs_f64();
            if seconds > 0.0 {
                state.metrics.histogram_observe(
                    &synra_observe::metrics::SeriesKey::new(
                        "transfer_throughput_bps",
                        [("kind".to_owned(), kind.to_owned())],
                    ),
                    byte_length as f64 / seconds,
                );
            }
        }
    }
}

/// Why a block could not be sent: the link refused it, or the user stopped the transfer.
#[derive(Debug)]
enum SendStop {
    /// The user cancelled; the loop stops before writing the next block.
    Cancelled,
    /// The link refused the block.
    Link(synra_core::LinkError),
}

impl std::fmt::Display for SendStop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("the transfer was cancelled"),
            Self::Link(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for SendStop {}

/// Starts a send on its own thread, and answers the transfer's identifier now.
///
/// # Why a thread rather than an awaited command
///
/// Because the dial and the block loop can take minutes, and a command that answers only at the end cannot be cancelled —
/// the answer is the only thing the interface ever gets, and by then there is nothing left to stop. The row and the token
/// exist before this returns, so a cancel arriving immediately has something to cancel.
///
/// # What the thread owns
///
/// The sender (and so the file's blocks), the row it reports, and the temporary file this shell wrote when there was one.
/// The file is removed on every exit path: a message stuck behind a dead link must not stay in the cache for ever.
pub fn start_send(
    state: &std::sync::Arc<crate::state::AppState>,
    to: synra_protocol::ids::DeviceId,
    address: std::net::SocketAddr,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
    temp: Option<std::path::PathBuf>,
    cleanup_dir: Option<std::path::PathBuf>,
) -> synra_protocol::ids::TransferId {
    let transfer_id = row.transfer_id;
    let cancel = synra_core::supervisor::Cancellation::new();
    state.transfers.track_cancel(transfer_id, cancel.clone());
    if let Some(path) = &temp {
        state.transfers.record_with_temp(row.clone(), path.clone());
    }
    announce_transfer(state, row.clone());

    let thread_state = std::sync::Arc::clone(state);
    let spawned = std::thread::Builder::new()
        .name(format!("synra-send-{transfer_id}"))
        .spawn(move || {
            if let Err(error) = send_over_with(&thread_state, to, address, sender, row, Some(&cancel)) {
                eprintln!("synra: transfer {transfer_id} did not finish: {error:?}");
            }
            if let Some(path) = temp {
                let _ = std::fs::remove_file(path);
            }
            if let Some(directory) = cleanup_dir {
                // ★ A directory, not a recursive delete: the worker knows the one file it was given and the directory
                // the uploader made for it. `remove_dir` refuses if anything else is inside, which is the right answer.
                let _ = std::fs::remove_dir(directory);
            }
        });
    if spawned.is_err() {
        // The row was announced before the thread existed; without this it would stay "queued" for ever.
        finish_transfer(state, transfer_id);
    }
    transfer_id
}

/// Opens a file and builds the row a send starts from.
///
/// # Why this is separate from starting the thread
///
/// Because the page's chunked `transfer.finish` and the desktop's `transfer_send_file` both need exactly this pair, and a
/// second copy would be a second place for "the row's file name is the path's last component" to be forgotten.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] for an unknown device, [`ErrorCode::InvalidArgument`] for a file that cannot be planned.
pub fn prepare_file_send(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    path: &str,
) -> Result<
    (std::net::SocketAddr, synra_core::transfer::TransferSender, TransferSnapshot),
    IpcError,
> {
    use synra_protocol::ErrorCode;

    let address = address_of(state, to)?;
    let sender = synra_core::transfer::TransferSender::open(
        std::path::Path::new(path),
        synra_core::transfer::DEFAULT_CHUNK_SIZE,
    )
    .map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;
    let planned = sender.request();

    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(&planned, (to, &to.to_string()), "queued", now_ms);
    row.file_name = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned());
    Ok((address, sender, row))
}

/// Prepares and starts a file send, answering the transfer's identifier.
///
/// # Errors
///
/// The same refusals as [`transfer_send_file`], all of which happen before a thread exists.
pub fn transfer_send_file_start(
    state: &std::sync::Arc<crate::state::AppState>,
    request: &synra_ipc::dto::SendFileRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    let (address, sender, row) = prepare_file_send(state, request.to, &request.path)?;
    Ok(start_send(state, request.to, address, sender, row, None, None))
}

/// Prepares and starts a text send, answering the transfer's identifier.
///
/// # Errors
///
/// As [`transfer_send_text`].
pub fn transfer_send_text_start(
    state: &std::sync::Arc<crate::state::AppState>,
    request: &synra_ipc::dto::SendTextRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let address = address_of(state, request.to)?;
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

    Ok(start_send(state, request.to, address, sender, row, Some(path), None))
}

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
        }
        else {
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
    }
    else {
        u32::try_from(request.byte_length.div_ceil(u64::from(request.chunk_size))).unwrap_or(u32::MAX)
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

/// Answers the prompt for an incoming transfer.
///
/// # The `remember` flag
///
/// `Remember` is written **before** the answer is delivered, so the very next transfer from the same device finds the
/// preference. A refusal is never remembered: "no" is about this file, not about the device.
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
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"transferId": transfer_id, "detail": error.to_string()}))
    })?;
    if accept
        && remember.unwrap_or(false)
        && let Some(peer) = state.transfer_desk.peer_of(&id.to_string())
    {
        remember_peer(state, &peer);
    }
    if state.transfer_desk.respond(&id.to_string(), accept) {
        return Ok(());
    }
    Err(IpcError::unknown(synra_protocol::ErrorCode::TransferFailed).with_params(serde_json::json!({
        "transferId": transfer_id,
        "reason": "it is not waiting for an answer",
    })))
}

/// The configuration key holding the devices this one accepts without asking.
const ACCEPT_FROM_KEY: &str = "transfers.acceptFrom";

/// The devices this one accepts without asking.
fn accepted_senders(state: &crate::state::AppState) -> Vec<String> {
    state
        .db()
        .config()
        .ok()
        .and_then(|rows| rows.into_iter().find(|row| row.key == ACCEPT_FROM_KEY))
        .and_then(|row| serde_json::from_str::<Vec<String>>(&row.value).ok())
        .unwrap_or_default()
}

/// Whether the user has chosen to accept everything from this device.
///
/// A sender that is not listed is asked about, which is the default and the only state a fresh install has.
#[must_use]
pub fn auto_accept_peer(state: &crate::state::AppState, peer: &str) -> bool {
    accepted_senders(state).iter().any(|accepted| accepted == peer)
}

/// Forgets a device the user chose to accept without asking.
///
/// Called when the device itself is forgotten: a trusted sender that is no longer paired is a preference about a device
/// this one no longer knows, and keeping it would silently re-trust the same identifier if it ever came back.
pub(crate) fn forget_sender(state: &crate::state::AppState, peer: &str) {
    let mut accepted = accepted_senders(state);
    let before = accepted.len();
    accepted.retain(|known| known != peer);
    if accepted.len() == before {
        return;
    }
    let now = synra_store::sqlite::now_ms();
    if accepted.is_empty() {
        // Unset rather than store an empty array: an empty preference is no preference, and a row saying "none" is a
        // setting a screen would have to interpret.
        let _ = state.db().apply_config(
            &[synra_store::ConfigChange { key: ACCEPT_FROM_KEY.to_owned(), value: None }],
            now,
        );
    }
    else {
        let value = serde_json::to_value(&accepted).unwrap_or_else(|_| serde_json::json!([]));
        let _ = state.db().set_config(ACCEPT_FROM_KEY, &value, now);
    }
}

/// Remembers a device the user chose to accept without asking.
///
/// Stored under one configuration key as a JSON array of device ids, so the settings screen can show (and one day remove)
/// it without a table of its own. A write that fails is silent: the answer still settles the current transfer, and the
/// next one asks again — the failure mode of a preference is another prompt, not a lost file.
fn remember_peer(state: &crate::state::AppState, peer: &str) {
    let mut accepted = accepted_senders(state);
    if accepted.iter().any(|known| known == peer) {
        return;
    }
    accepted.push(peer.to_owned());
    let value = serde_json::to_value(&accepted).unwrap_or_else(|_| serde_json::json!([]));
    let _ = state.db().set_config(ACCEPT_FROM_KEY, &value, synra_store::sqlite::now_ms());
}

/// Closes a receive row whose transfer ended without a file.
///
/// The row was opened by the progress reporter when the proposal was accepted, and a sender that aborts never produces a
/// `FinishedTransfer` — so this is the only place that can tell the panel the row is over. It is not `completed`: nothing
/// arrived.
pub fn withdraw_received(state: &crate::state::AppState, transfer_id: TransferId) {
    state.transfers.forget(&transfer_id);
    state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
}

/// Announces a received transfer as completed and retires its row.
///
/// The receiving half of a transfer has no worker to remove the row; without this the panel would show it as arriving
/// for ever. A transfer that never registered a row is still finished: the announcement is how the panel learns the id
/// is done.
pub fn complete_received(
    state: &crate::state::AppState,
    transfer_id: synra_protocol::ids::TransferId,
) {
    // The registry may not have the row — a transfer whose first blocks arrived before the reporter attached, or one
    // whose connection ended badly. Finishing it is still right: the announcement is how the panel learns the id is done.
    if let Some(mut snapshot) = state
        .transfers
        .list()
        .into_iter()
        .find(|row| row.transfer_id == transfer_id)
    {
        snapshot.state = "completed".to_owned();
        snapshot.received_bytes = snapshot.total_bytes;
        snapshot.blocks_done = snapshot.blocks_total;
        announce_transfer(state, snapshot);
    }
    finish_transfer(state, transfer_id);
}

/// Removes a transfer and tells the interface it is over.
///
/// One function because the two halves must not be separated: a row removed without the announcement leaves the panel
/// showing a transfer that no longer exists, and an announcement without the removal leaves one it can never clear.
pub(crate) fn finish_transfer(state: &crate::state::AppState, transfer_id: synra_protocol::ids::TransferId) {
    state.transfers.forget(&transfer_id);
    state.transfers.untrack(&transfer_id);
    state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
}

/// The wall clock, in milliseconds, as the counts this module reports are measured.
///
/// Visible to the other command modules because they report the same kind of timestamps and a second copy is a second
/// place for "milliseconds" to become seconds.
pub(crate) fn unix_millis() -> u64 {
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
