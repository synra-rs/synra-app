use super::*;

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
    state.transfers.record_owned(row.clone(), temp, cleanup_dir);
    announce_transfer(state, row.clone());

    let thread_state = std::sync::Arc::clone(state);
    let spawned = std::thread::Builder::new()
        .name(format!("synra-send-{transfer_id}"))
        .spawn(move || {
            if let Err(error) =
                send_over_with(&thread_state, to, address, sender, row, Some(&cancel))
            {
                eprintln!("synra: transfer {transfer_id} did not finish: {error:?}");
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
    (
        std::net::SocketAddr,
        synra_core::transfer::TransferSender,
        TransferSnapshot,
    ),
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
    row.file_name = std::path::Path::new(path).file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
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
    Ok(start_send(
        state, request.to, address, sender, row, None, None,
    ))
}

/// Where a text send's temporary file goes.
///
/// # Why the request's name is used, with a timestamp
///
/// Because the receiver names the arriving file after this path, and `message-<ms>.txt` threw away the one thing a person
/// recognises. The stem is sanitised rather than trusted: it comes from the interface, and a name with a separator in it
/// must not become a path. The timestamp keeps two sends of the same name from overwriting each other before the worker
/// removes the file.
pub(super) fn outbox_path(
    outbox: &std::path::Path,
    file_name: &str,
    now_ms: u64,
) -> std::path::PathBuf {
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(synra_store::sanitise_component)
        .filter(|stem| !stem.is_empty() && stem != "_")
        .unwrap_or_else(|| "message".to_owned());
    outbox.join(format!("{stem}-{now_ms}.txt"))
}
