use super::*;

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
    state.publish(synra_ipc::event::KernelEvent::TransferUpdated(Box::new(
        snapshot,
    )));
    // The identifier is kept above so the event and the record cannot name different transfers, which is the failure a
    // second read of `snapshot.transfer_id` after the move would allow.
    debug_assert_eq!(
        state
            .transfers
            .list()
            .iter()
            .filter(|t| t.transfer_id == transfer_id)
            .count(),
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
    let blocks_total = u32::try_from(request.byte_length.div_ceil(u64::from(request.chunk_size)))
        .unwrap_or(u32::MAX);

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

/// The files that have landed in this device's inbox, newest first.
///
/// # Why this scans the inbox instead of remembering
///
/// Because the inbox is where the file is. A table of received files that outlived the directory would send a user to a
/// path that no longer exists, and a directory that outlived the table is the file the user is looking for. Package
/// bundles are skipped: they were installed, and the plugins page is their history.
///
/// # Errors
///
/// Never in practice: an inbox that cannot be read is an empty history, which is what a fresh installation has.
pub fn transfer_received(state: &crate::state::AppState) -> Vec<synra_ipc::dto::ReceivedFile> {
    let inbox = state.paths.data_dir().join(synra_core::inbound::INBOX_DIR);
    let Ok(entries) = std::fs::read_dir(&inbox) else {
        return Vec::new();
    };

    let mut files: Vec<synra_ipc::dto::ReceivedFile> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("synra"))
            {
                return None;
            }
            let received_at_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| {
                    i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
                });
            Some(synra_ipc::dto::ReceivedFile {
                file_name: path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                path: path.display().to_string(),
                bytes: metadata.len(),
                received_at_ms,
            })
        })
        .collect();

    // Newest first: the file a person is looking for is almost always the one that just arrived.
    files.sort_by_key(|file| std::cmp::Reverse(file.received_at_ms));
    files.truncate(50);
    files
}

/// Resolves a received-file path, refusing anything outside the inbox.
///
/// # Why the containment check is here rather than in the interface
///
/// The path comes from the bridge, and `open_path` launches whatever the operating system associates with it. A webview
/// that could name `~/.ssh` would turn "open the file I received" into "open anything on this disk" — the check has to
/// be on the side that knows where received files live.
pub(super) fn inbox_file(
    state: &crate::state::AppState,
    path: &str,
) -> Result<std::path::PathBuf, synra_ipc::IpcError> {
    let inbox = state.paths.data_dir().join(synra_core::inbound::INBOX_DIR);
    let root = inbox.canonicalize().map_err(|error| {
        synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": format!("the inbox is not readable: {error}")}))
    })?;
    let file = std::path::Path::new(path).canonicalize().map_err(|error| {
        synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err(
            synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"path": path, "reason": "it is not a file in the inbox"}),
            ),
        );
    }
    Ok(file)
}

/// Opens a received file with the platform's default application.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path that is not a file in the inbox, and `Unsupported` when the platform refuses
/// to launch it. The refusal carries the reason; opening a file is the one action where "nothing happened" is hardest to
/// diagnose from the screen alone.
pub fn transfer_open(
    state: &crate::state::AppState,
    path: &str,
) -> Result<(), synra_ipc::IpcError> {
    use synra_platform::Platform as _;

    let file = inbox_file(state, path)?;
    state.platform.open_path(&file).map_err(|error| {
        synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })
}

/// Shows a received file in the platform's file manager.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path that is not a file in the inbox, and `Unsupported` when the platform refuses
/// to reveal it. The containment check is the same one [`transfer_open`] uses, because revealing a path tells the user
/// what exists on their disk just as much as opening it does.
pub fn transfer_reveal(
    state: &crate::state::AppState,
    path: &str,
) -> Result<(), synra_ipc::IpcError> {
    use synra_platform::Platform as _;

    let file = inbox_file(state, path)?;
    state.platform.reveal_path(&file, None).map_err(|error| {
        synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })
}

/// Deletes a received file from the inbox.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path that is not a file in the inbox, and [`ErrorCode::Internal`] when the file
/// exists but cannot be removed. The containment check is the same one Open and Reveal use: a command that deletes by path
/// must not be able to delete anything the user did not receive.
pub fn transfer_discard(
    state: &crate::state::AppState,
    path: &str,
) -> Result<(), synra_ipc::IpcError> {
    let file = inbox_file(state, path)?;
    std::fs::remove_file(&file).map_err(|error| {
        synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::Internal)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })
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
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"transferId": transfer_id, "detail": error.to_string()}),
        )
    })?;

    if state
        .transfers
        .list()
        .iter()
        .all(|transfer| transfer.transfer_id != id)
    {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::TransferFailed).with_params(
                serde_json::json!({"transferId": transfer_id, "reason": "it is not in flight"}),
            ),
        );
    }
    // ★ Two directions, two mechanisms. An outgoing transfer has a worker holding a cancellation token; an incoming one
    // has a connection thread that owns the link, so the command can only leave the identifier where that loop will look.
    if state.transfers.stop(&id) {
        finish_transfer(state, id);
    } else {
        // The row stays: the listener has not seen the request yet, and removing it here would make the interface forget a
        // transfer that is still running. It goes when the listener's abort reaches the progress callback.
        state.inbound_cancels.lock().insert(id.to_string());
    }
    Ok(())
}
