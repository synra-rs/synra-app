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

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager as _;

use crate::state::AppState;

/// Binds the port this device is reachable on, and records it in the state.
///
/// # Why this is separate from [`spawn`]
///
/// Because discovery announces the port a peer should **dial**, and it starts before the window is managed. Binding first is what
/// lets the announcement carry the listener's port rather than the discovery responder's — the difference between a peer reaching
/// this device and a peer reaching a service that only answers datagrams.
///
/// # What a failed bind costs
///
/// The reachability, and nothing else. The window opens, the scan works, and `pairing_request` reports the failure of a dial rather
/// than the interface losing itself.
#[must_use]
pub fn bind(mut state: AppState) -> AppState {
    // ★ **Once**, and into a local, before anything else touches the identity.
    //
    // This was a self-deadlock: `Host::new(state.identity.lock().clone(), local_of(&state))` holds the guard from the first
    // argument until the end of the statement — Rust keeps a temporary alive across the whole expression — and `local_of`
    // locks the same `parking_lot::Mutex` again. That mutex is **not reentrant**, so `setup` never returned, the window never
    // loaded, and the only sign was a white screen with discovery's log above it.
    let identity = state.identity.lock().clone();
    // ★ The second host this process builds, and it needs the recorder for the same reason the state's host does: an
    // accepted connection is a link, and its frames are the only evidence the listener ever handled anything.
    let host = Arc::new(Mutex::new(
        synra_core::Host::new(identity.clone(), local_of(&identity))
            .with_metrics(Arc::clone(&state.metrics)),
    ));

    let bound = {
        let mut guard = host.lock();
        synra_core::listener::bind(&mut guard, 0).map(|address| address.port())
    };

    match bound {
        Ok(port) => {
            state.listener = Some((host, port));
        }
        Err(error) => {
            eprintln!("synra: the listener could not bind a port: {error}");
        }
    }
    state
}

/// Starts the listener, and answers the token that stops it.
///
/// The token is returned rather than dropped so a caller can stop the listener on the way out. A thread whose token is thrown away
/// is one that can only be stopped by killing the process.
#[must_use]
pub fn spawn(app: &tauri::AppHandle) -> Option<synra_core::supervisor::Cancellation> {
    let state = app.state::<Arc<AppState>>();
    let Some((host, port)) = state.listener.clone() else {
        eprintln!("synra: no port was bound, so this window cannot be reached");
        return None;
    };

    let cancel = synra_core::supervisor::Cancellation::new();
    let thread_cancel = cancel.clone();
    let data_dir = state.paths.data_dir().to_path_buf();
    // ★ The window writes late answers to the same disk queue the daemon uses, so it has to drain it too; otherwise a
    // window that could not reach a peer would leave the answer on disk for ever. The retry thread stops on the same
    // cancellation token as the listener.
    {
        let host = Arc::clone(&host);
        let data_dir = data_dir.clone();
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            loop {
                let _ = synra_core::listener::retry_late(&host, &data_dir);
                if cancel.wait(std::time::Duration::from_secs(30)) {
                    break;
                }
            }
        });
    }
    // ★ The desk comes **from the state**, not from here. A request arrives on a socket thread and the user answers from a
    // command; if the listener made its own desk, the command's answer would land on a different one and the connection would
    // wait out its window for a decision already made.
    let desk = Arc::clone(&state.pairing);
    // ★ The router the kernel's listener wants is a thin pair of the Arcs this shell already holds — not the daemon's 660-line
    // orchestration. It needs a plugin set, and `PluginSet::new` refuses to exist without a host to route effects through, so a
    // window with no plugin host does not listen at all: one that accepted a share and could route it nowhere would be a link
    // that answers "yes" and then does nothing.
    let Some(plugins) = state.plugins.clone() else {
        eprintln!("synra: this window has no plugin host, so it does not listen");
        return None;
    };
    let router = Arc::new(synra_core::dispatch::orchestrate::Router::new(
        plugins,
        Arc::clone(&state.board),
    ));

    // Cloned before the thread starts: the guard borrows the application handle, and a thread outlives that borrow.
    let db_state: Arc<AppState> = Arc::clone(state.inner());

    std::thread::Builder::new()
        .name(format!("synra-listener-{port}"))
        .spawn(move || {
            // ★ The kernel's own half, not a second implementation of it. The daemon has used this function since it
            // learned to record what arrives; the window used to record only "the peer holds this", return nothing, and
            // leave the sender waiting out its deadline for a result nobody sent.
            // ★ When each inbound transfer was first seen. The finish callback knows the size but not when it started, and
            // the progress callback is the only place that knows the first block arrived; one map shared by both closes
            // that gap. A transfer that aborts before any progress has no start, and records no duration rather than one
            // measured from a guess.
            let inbound_started: Arc<
                Mutex<std::collections::HashMap<synra_protocol::ids::TransferId, std::time::Instant>>,
            > = Arc::new(Mutex::new(std::collections::HashMap::new()));

            let finished_state = Arc::clone(&db_state);
            let finished_started = Arc::clone(&inbound_started);
            let finished: synra_core::listener::FinishedCallback = Arc::new(
                move |transfer: &synra_core::listener::FinishedTransfer| {
                    // ★ The bytes are a `.synra` package now, so receiving one is **installing** it: the same pipeline the
                    // install command runs, over the archive the peer sent. `record_received` then writes the history and
                    // the sync state — and its plugin-row insert fails harmlessly, because the installer already made one
                    // with a version attached.
                    let outcome = {
                        let mut db = finished_state.db();
                        match &transfer.bundle {
                            Some(plugin_id) => match crate::commands::plugins::install_package(
                                &finished_state,
                                &mut db,
                                &transfer.path,
                            ) {
                                Ok(_) => synra_core::plugin_sync::record_received(&db, transfer),
                                Err(error) => Some(synra_core::plugin_sync::SyncOutcome::refused(
                                    plugin_id.clone(),
                                    synra_core::plugin_sync::SyncSessionId::new(
                                        transfer
                                            .sync_session_id
                                            .map_or_else(String::new, |session| session.to_string()),
                                    ),
                                    error,
                                )),
                            },
                            None => synra_core::plugin_sync::record_received(&db, transfer),
                        }
                    };
                    // ★ And the row this side opened when the proposal arrived. The receiving half of a transfer has no
                    // worker to remove it; without this the panel would show a transfer as arriving for ever.
                    if let Some(transfer_id) = transfer.transfer_id {
                        crate::commands::transfers::complete_received(&finished_state, transfer_id);
                    }
                    // ★ The receiving half's measurement, beside the row it closes. The kind comes from the same fact
                    // the install branch above used, so a bundle is counted as a bundle.
                    let started = transfer
                        .transfer_id
                        .and_then(|transfer_id| finished_started.lock().remove(&transfer_id));
                    crate::commands::transfers::record_received(
                        &finished_state,
                        if transfer.bundle.is_some() { "plugin-bundle" } else { "attachment" },
                        transfer.byte_length,
                        started.map(|started| started.elapsed()),
                        false,
                    );
                    outcome
                },
            );

            // The peer's name is looked up once per connection and kept: a per-block query would be a database read for
            // every 64 KiB, and a row that showed an identifier for the first block and a name for the second would look
            // like two devices.
            let progress_state = Arc::clone(&db_state);
            let progress_started = Arc::clone(&inbound_started);
            let names: Arc<Mutex<std::collections::HashMap<String, String>>> =
                Arc::new(Mutex::new(std::collections::HashMap::new()));
            let progress: Arc<synra_core::listener::ProgressCallback> = Arc::new(move |event| {
                // ★ A sender's abort is the last event of a transfer that produced no file. The row is removed rather than
                // marked completed: nothing arrived, and a "completed" row for a file that does not exist is a lie the
                // panel would keep until the next restart.
                if event.aborted {
                    // ★ Counted before the row goes: "someone sent and it did not land" is a transfer, and the size is
                    // deliberately not recorded — this histogram describes files that arrived.
                    let started = progress_started.lock().remove(&event.progress.transfer_id);
                    crate::commands::transfers::record_received(
                        &progress_state,
                        if event.progress.bundle.is_some() { "plugin-bundle" } else { "attachment" },
                        event.progress.received_bytes,
                        started.map(|started| started.elapsed()),
                        true,
                    );
                    crate::commands::transfers::withdraw_received(
                        &progress_state,
                        event.progress.transfer_id,
                    );
                    return;
                }
                // The first event for a transfer is its start; later blocks leave the time alone.
                progress_started
                    .lock()
                    .entry(event.progress.transfer_id)
                    .or_insert_with(std::time::Instant::now);
                let key = event.peer.to_string();
                let name = {
                    let mut names = names.lock();
                    if let Some(name) = names.get(&key) {
                        name.clone()
                    }
                    else {
                        let looked_up = progress_state
                            .db()
                            .paired_devices()
                            .ok()
                            .and_then(|devices| devices.into_iter().find(|device| device.device_id == key))
                            .map_or_else(|| key.clone(), |device| device.display_name);
                        names.insert(key.clone(), looked_up.clone());
                        looked_up
                    }
                };
                let snapshot = crate::commands::transfers::received_snapshot(
                    event,
                    &name,
                    synra_store::sqlite::now_ms(),
                );
                crate::commands::transfers::announce_transfer(&progress_state, snapshot);
            });

            // ★ The question a peer's file asks. Attachments wait for a person; plugin bundles do not, because a sync round
            // is already a decision the user made by pressing "sync", and a prompt in the middle of it would make the round
            // depend on somebody watching the window.
            let offer_state = Arc::clone(&db_state);
            let offer_desk = Arc::clone(&db_state.transfer_desk);
            let offer: Arc<synra_core::inbound::OfferCallback> = Arc::new(
                move |incoming: &synra_core::inbound::Offer<'_>| {
                    use synra_core::inbound::OfferDecision;
                    if !matches!(incoming.request.kind, synra_protocol::session::TransferKind::Attachment) {
                        return OfferDecision::Accept;
                    }
                    // ★ A sender the user chose to keep accepting is not asked about again. The check is before anything
                    // is announced, so a remembered device does not flash a prompt on its way past.
                    if crate::commands::transfers::auto_accept_peer(
                        &offer_state,
                        &incoming.peer.to_string(),
                    ) {
                        return OfferDecision::Accept;
                    }
                    let now = synra_store::sqlite::now_ms();
                    let peer_name = offer_state
                        .db()
                        .paired_devices()
                        .ok()
                        .and_then(|devices| {
                            devices
                                .into_iter()
                                .find(|device| device.device_id == incoming.peer.to_string())
                        })
                        .map_or_else(|| incoming.peer.to_string(), |device| device.display_name);
                    let pending = crate::transfer_desk::Pending {
                        transfer_id: incoming.request.transfer_id.to_string(),
                        peer: incoming.peer.to_string(),
                        file_name: incoming.file_name.to_owned(),
                        total_bytes: incoming.request.byte_length,
                        started_at_ms: now,
                    };
                    let announced =
                        crate::commands::transfers::offered_snapshot(incoming, &peer_name, now);
                    let show = || {
                        crate::commands::transfers::announce_transfer(&offer_state, announced);
                    };
                    let accepted = offer_desk.ask(
                        &pending,
                        show,
                        crate::transfer_desk::OFFER_TIMEOUT,
                    );
                    crate::commands::transfers::withdraw_offer(
                        &offer_state,
                        incoming.request.transfer_id,
                    );
                    if accepted {
                        OfferDecision::Accept
                    }
                    else {
                        OfferDecision::Refuse
                    }
                },
            );

            let cancels = Arc::clone(&db_state.inbound_cancels);
            let should_cancel: Arc<synra_core::listener::CancelCallback> =
                Arc::new(move |id: &str| cancels.lock().remove(id));

            // ★ The window is a dispatch target too: a peer that chose this device sends `INTENT_INVOKE` here, and the
            // answer has to go back on this connection. The daemon had this callback first; without it here, a remote
            // action aimed at the window was counted and dropped — the sender stayed "dispatching" for ever.
            let kernel_state = Arc::clone(&db_state);
            let on_kernel_events: Arc<synra_core::listener::KernelEventCallback> = Arc::new(
                move |envelope: &synra_protocol::envelope::Envelope| {
                    use synra_core::listener::KernelEventOutcome;
                    use synra_protocol::event::events;
                    let board = Arc::clone(&kernel_state.board);
                    let now = synra_store::sqlite::now_ms();
                    let local = kernel_state.identity.lock().device_id;
                    match envelope.event.as_str() {
                        events::INTENT_INVOKE => {
                            // ★ The engine is looked up **inside** this arm: a late result and a withdrawal only need the
                            // board, and refusing them because this window cannot run plugins would drop an outcome for an
                            // action the peer already performed.
                            let Some(plugins) = kernel_state.plugins.as_ref().map(Arc::clone) else {
                                return KernelEventOutcome::Refused(
                                    "this window has no plugin engine, so it cannot run the action"
                                        .to_owned(),
                                );
                            };
                            match synra_core::dispatch::orchestrate::accept_remote(
                                &plugins, &board, local, envelope, now,
                            ) {
                                Ok(Some(reply)) => KernelEventOutcome::Reply(Box::new(reply)),
                                // The same request twice with the first copy still running: the first copy answers.
                                Ok(None) => KernelEventOutcome::Handled,
                                Err(error) => KernelEventOutcome::Refused(error.to_string()),
                            }
                        }
                        events::INTENT_CANCEL => {
                            match synra_core::dispatch::orchestrate::accept_cancel(&board, envelope) {
                                Ok(_) => KernelEventOutcome::Handled,
                                Err(error) => KernelEventOutcome::Refused(error.to_string()),
                            }
                        }
                        // The late answer to an action this window dispatched: the receiver waited longer than this side
                        // did and dialled back. `apply_result` ignores one for work no longer in flight.
                        events::INTENT_RESULT => {
                            match synra_core::dispatch::orchestrate::apply_result(&board, envelope) {
                                Ok(_) => KernelEventOutcome::Handled,
                                Err(error) => KernelEventOutcome::Refused(error.to_string()),
                            }
                        }
                        _ => KernelEventOutcome::Ignored,
                    }
                },
            );

            synra_core::listener::serve_with(
                &host,
                &thread_cancel,
                &data_dir,
                &desk,
                &router,
                &finished,
                &progress,
                &offer,
                &should_cancel,
                &on_kernel_events,
            );
        })
        .map_or_else(
            |error| {
                eprintln!("synra: the listener thread could not start: {error}");
                None
            },
            |_| Some(cancel),
        )
}

/// What this device says about itself on the wire.
///
/// It takes the identity rather than the state: a version that locked the state would be the second caller of a lock its own caller
/// already holds, and `parking_lot`'s mutex is not reentrant. Passing the value makes that impossible to write by accident.
fn local_of(identity: &synra_core::identity::Identity) -> synra_core::peer::LocalPeer {
    synra_core::peer::LocalPeer::new(
        identity.device_id,
        identity.display_name.clone(),
        synra_protocol::session::PlatformKind::Desktop,
        env!("CARGO_PKG_VERSION"),
    )
}
