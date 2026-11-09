use super::*;

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
        IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"path": outbox.display().to_string(), "reason": error.to_string()}),
        )
    })?;
    let path = outbox_path(&outbox, &request.file_name, unix_millis());
    std::fs::write(&path, request.text.as_bytes()).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()}),
        )
    })?;

    let sender = synra_core::transfer::TransferSender::open(
        &path,
        synra_core::transfer::DEFAULT_CHUNK_SIZE,
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&path);
    })
    .map_err(|error| {
        let params =
            serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()});
        IpcError::unknown(ErrorCode::InvalidArgument).with_params(params)
    })?;

    let planned = sender.request();
    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(
        &planned,
        (request.to, &request.to.to_string()),
        "queued",
        now_ms,
    );
    row.kind = "attachment".to_owned();

    Ok(start_send(
        state,
        request.to,
        address,
        sender,
        row,
        Some(path),
        None,
    ))
}

/// Sends text as a file, and answers the transfer it started.
///
/// # Why text is a file
///
/// Because the transport has no notion of one: [`TransferSender`] opens a path, and the wire's metadata has `Attachment`
/// and `PluginBundle` and nothing else. The alternative — a new kind for text — would be a protocol change to save writing one
/// file and deleting it again.
///
/// The temporary file is tracked by the transfer registry and removed when the attempt ends or is cancelled.
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
        IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"path": outbox.display().to_string(), "reason": error.to_string()}),
        )
    })?;
    let path = outbox_path(&outbox, &request.file_name, unix_millis());
    std::fs::write(&path, request.text.as_bytes()).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()}),
        )
    })?;

    let sender = synra_core::transfer::TransferSender::open(
        &path,
        synra_core::transfer::DEFAULT_CHUNK_SIZE,
    )
    .inspect_err(|_| {
        // ★ Removed here as well as after the send: a sender that refused the file must not leave it on disk for the next
        // run to find, and the delete below is only reached when this call succeeded.
        let _ = std::fs::remove_file(&path);
    })
    .map_err(|error| {
        let params =
            serde_json::json!({"path": path.display().to_string(), "reason": error.to_string()});
        IpcError::unknown(ErrorCode::InvalidArgument).with_params(params)
    })?;

    let planned = sender.request();
    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(
        &planned,
        (request.to, &request.to.to_string()),
        "queued",
        now_ms,
    );
    row.kind = "attachment".to_owned();

    // Registered with its file before the send starts, so a cancel that arrives mid-send has something to delete.
    state.transfers.record_with_temp(row.clone(), path.clone());
    let outcome = send_over(state, request.to, address, sender, row);
    // Every exit path, including the one where the link refused every block.
    let _ = std::fs::remove_file(&path);
    outcome
}
