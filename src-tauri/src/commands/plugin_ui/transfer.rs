use super::*;
use crate::commands::transfers;

pub(crate) const MAX_PLUGIN_TRANSFER_BYTES: usize = 4 * 1024 * 1024;

/// The largest file a page may send as a **chunked** upload.
///
/// The single-call cap is a property of one IPC message, not of a transfer: a chunked upload is how a page sends a file
/// larger than that, and 64 MiB is the design's whole-package ceiling (16 §2.1).
pub(crate) const MAX_PLUGIN_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// The largest chunk one `transfer.chunk` may carry.
///
/// One mebibyte of bytes is about 1.4 MiB of base64 in the RPC payload: large enough that a 64 MiB file is 64 round trips
/// rather than hundreds, small enough that a chunk is not a whole message-budget.
pub(crate) const MAX_PLUGIN_CHUNK_BYTES: usize = 1024 * 1024;

/// A chunked upload a page is writing.
#[derive(Debug)]
pub(crate) struct Upload {
    /// Which plugin started it; only that plugin may append to or finish it.
    plugin_id: String,
    /// The directory this upload owns, removed with it.
    directory: std::path::PathBuf,
    /// The file being written.
    path: std::path::PathBuf,
    /// Who it is for.
    to: synra_protocol::ids::DeviceId,
    /// How many bytes the page declared.
    total: u64,
    /// How many have been written so far.
    written: u64,
}

/// The chunked uploads in progress, by the identifier `transfer.begin` answered.
#[derive(Debug, Default)]
pub struct Uploads {
    by_id: parking_lot::Mutex<std::collections::HashMap<String, Upload>>,
}

impl Uploads {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, id: String, upload: Upload) {
        self.by_id.lock().insert(id, upload);
    }

    /// Removes an upload, but only for the plugin that started it.
    fn take(&self, plugin_id: &str, id: &str) -> Option<Upload> {
        let mut uploads = self.by_id.lock();
        if uploads
            .get(id)
            .is_some_and(|upload| upload.plugin_id == plugin_id)
        {
            uploads.remove(id)
        } else {
            None
        }
    }
}

/// Removes the file and directory a chunked upload owned.
pub(crate) fn discard_upload(upload: &Upload) {
    let _ = std::fs::remove_file(&upload.path);
    let _ = std::fs::remove_dir(&upload.directory);
}

/// Starts a chunked transfer and answers the identifier the page appends with.
pub(crate) fn transfer_begin(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    use synra_protocol::ids::DeviceId;

    require_grant(session, "file:transfer:send", "transfer.begin")?;
    let to = uuid::Uuid::parse_str(string_arg(args, "to")?)
        .map(DeviceId::from_uuid)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": "to", "reason": error.to_string()}))
        })?;
    let file_name = string_arg(args, "fileName")?;
    transfer_file_name(file_name)?;
    let total = args
        .get("totalBytes")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"field": "totalBytes", "reason": "a transfer needs a size"}),
            )
        })?;
    if total == 0 {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"field": "totalBytes", "reason": "the file is empty"}),
        ));
    }
    if total > MAX_PLUGIN_UPLOAD_BYTES {
        return Err(
            IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(serde_json::json!({
                "field": "totalBytes",
                "bytes": total,
                "maxBytes": MAX_PLUGIN_UPLOAD_BYTES,
            })),
        );
    }

    let directory = state
        .paths
        .cache_dir()
        .join("plugin-transfers")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": format!("could not make the transfer directory: {error}")}))
    })?;
    let path = directory.join(file_name);
    if let Err(error) = std::fs::File::create(&path) {
        let _ = std::fs::remove_dir(&directory);
        return Err(IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"detail": format!("could not create the transfer file: {error}")}),
        ));
    }

    let upload_id = uuid::Uuid::new_v4().to_string();
    state.uploads.insert(
        upload_id.clone(),
        Upload {
            plugin_id: session.plugin_id().to_string(),
            directory,
            path,
            to,
            total,
            written: 0,
        },
    );
    Ok(serde_json::json!({"uploadId": upload_id}))
}

/// Appends one chunk to a chunked transfer.
// The two lock scopes are deliberate: one reads the upload's path before the write, the other writes the counter back
// after it. The file I/O happens between them, which is what the lint's advice to shorten the guard would lose.
#[allow(clippy::significant_drop_tightening)]
pub(crate) fn transfer_chunk(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    use base64::Engine as _;
    use std::io::Write as _;

    require_grant(session, "file:transfer:send", "transfer.chunk")?;
    let upload_id = string_arg(args, "uploadId")?;
    let data = string_arg(args, "data")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": "data", "reason": error.to_string()}))
        })?;
    if bytes.len() > MAX_PLUGIN_CHUNK_BYTES {
        return Err(
            IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(serde_json::json!({
                "field": "data",
                "bytes": bytes.len(),
                "maxBytes": MAX_PLUGIN_CHUNK_BYTES,
            })),
        );
    }

    // ★ The lock is released before the file is written: a chunk is an IPC round trip and a write, and holding the map
    // for either would make one page's slow disk stall every other upload. The pair (path, declared total) is all the
    // write needs; the counter is reconciled under the lock afterwards.
    let (path, total, next) = {
        let uploads = state.uploads.by_id.lock();
        let Some(upload) = uploads.get(upload_id) else {
            return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"uploadId": upload_id, "reason": "no upload is waiting"}),
            ));
        };
        if upload.plugin_id != session.plugin_id().to_string() {
            return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"uploadId": upload_id, "reason": "no upload is waiting"}),
            ));
        }
        let next = upload.written.saturating_add(bytes.len() as u64);
        if next > upload.total {
            return Err(IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(
                serde_json::json!({
                    "uploadId": upload_id,
                    "written": next,
                    "totalBytes": upload.total,
                    "reason": "the chunk is larger than the size the page declared",
                }),
            ));
        }
        (upload.path.clone(), upload.total, next)
    };

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::Internal).with_params(
                serde_json::json!({"detail": format!("could not open the transfer file: {error}")}),
            )
        })?;
    file.write_all(&bytes).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"detail": format!("could not append the chunk: {error}")}),
        )
    })?;

    {
        let mut uploads = state.uploads.by_id.lock();
        let Some(upload) = uploads.get_mut(upload_id) else {
            return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"uploadId": upload_id, "reason": "the upload went away"}),
            ));
        };
        upload.written = next;
    }
    Ok(serde_json::json!({"written": next, "totalBytes": total}))
}

/// Finishes a chunked transfer: checks the size, sends it, and removes the page's file.
pub(crate) fn transfer_finish(
    state: &AppState,
    arc: Option<&std::sync::Arc<AppState>>,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "file:transfer:send", "transfer.finish")?;
    let upload_id = string_arg(args, "uploadId")?;
    let Some(upload) = state.uploads.take(session.plugin_id().as_str(), upload_id) else {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"uploadId": upload_id, "reason": "no upload is waiting"}),
        ));
    };
    if upload.written != upload.total {
        discard_upload(&upload);
        return Err(
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "uploadId": upload_id,
                "written": upload.written,
                "totalBytes": upload.total,
                "reason": "the file ended before the size the page declared",
            })),
        );
    }

    // ★ With the managed state, the send starts on a worker and this answers now — the same contract as the desktop's
    // `transfer_send_file`. The worker also owns the upload's file and directory, so nothing is left in the cache.
    if let Some(arc) = arc {
        let (address, sender, row) =
            match transfers::prepare_file_send(state, upload.to, &upload.path.to_string_lossy()) {
                Ok(prepared) => prepared,
                Err(error) => {
                    discard_upload(&upload);
                    return Err(error);
                }
            };
        let transfer_id = transfers::start_send(
            arc,
            upload.to,
            address,
            sender,
            row,
            Some(upload.path),
            Some(upload.directory),
        );
        return Ok(serde_json::json!({"transferId": transfer_id.to_string()}));
    }

    let outcome = transfers::transfer_send_file(
        state,
        &synra_ipc::dto::SendFileRequest {
            to: upload.to,
            path: upload.path.to_string_lossy().into_owned(),
            context_id: None,
        },
    );
    // ★ Both paths clean up: a failed dial must not leave the page's bytes in the cache directory.
    discard_upload(&upload);
    Ok(serde_json::json!({"transferId": outcome?.to_string()}))
}

/// Abandons a chunked transfer and removes what the page had written.
pub(crate) fn transfer_abort(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "file:transfer:send", "transfer.abort")?;
    let upload_id = string_arg(args, "uploadId")?;
    match state.uploads.take(session.plugin_id().as_str(), upload_id) {
        Some(upload) => {
            discard_upload(&upload);
            Ok(serde_json::json!({"aborted": true}))
        }
        None => Ok(serde_json::json!({"aborted": false})),
    }
}

/// Whether a page's file name is one path component a receiver may safely be told.
pub(crate) fn transfer_file_name(name: &str) -> Result<(), IpcError> {
    let refused = |reason: &str| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": "fileName", "reason": reason}))
    };
    if name.is_empty() {
        return Err(refused("a file needs a name"));
    }
    if name.len() > 255 {
        return Err(refused("the name is longer than 255 bytes"));
    }
    if name.contains(['/', '\\', '\0']) || name == "." || name == ".." {
        return Err(refused("the name must be one path component"));
    }
    Ok(())
}

/// The client's name for a transfer state.
///
/// The sender's snapshots say `queued` and `active`; the contract's union says `pending` and `transferring`. The rest are
/// the same word in both places, so they pass through instead of being mapped twice.
pub(crate) fn transfer_state_name(state: &str) -> &str {
    match state {
        "queued" => "pending",
        "active" => "transferring",
        other => other,
    }
}

/// Sends one file a page handed over as base64.
pub(crate) fn transfer_send(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    use base64::Engine as _;
    use synra_protocol::ids::DeviceId;

    require_grant(session, "file:transfer:send", "transfer.send")?;
    if args.get("contextId").is_some() {
        return Err(
            IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "reason": "grouping a transfer by context is not implemented on this channel yet",
            })),
        );
    }
    let to_text = string_arg(args, "to")?;
    let to = uuid::Uuid::parse_str(to_text)
        .map(DeviceId::from_uuid)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": "to", "reason": error.to_string()}))
        })?;
    let file_name = string_arg(args, "fileName")?;
    transfer_file_name(file_name)?;
    let data = string_arg(args, "data")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": "data", "reason": error.to_string()}))
        })?;
    if bytes.len() > MAX_PLUGIN_TRANSFER_BYTES {
        return Err(
            IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(serde_json::json!({
                "field": "data",
                "bytes": bytes.len(),
                "maxBytes": MAX_PLUGIN_TRANSFER_BYTES,
            })),
        );
    }

    // One directory per transfer: the file keeps the name the page chose, two sends cannot collide, and the cleanup
    // removes exactly what this call created.
    let directory = state
        .paths
        .cache_dir()
        .join("plugin-transfers")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": format!("could not make the transfer directory: {error}")}))
    })?;
    let path = directory.join(file_name);
    if let Err(error) = std::fs::write(&path, &bytes) {
        let _ = std::fs::remove_dir(&directory);
        return Err(IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"detail": format!("could not write the transfer file: {error}")}),
        ));
    }

    let outcome = transfers::transfer_send_file(
        state,
        &synra_ipc::dto::SendFileRequest {
            to,
            path: path.to_string_lossy().into_owned(),
            context_id: None,
        },
    );
    // ★ Both paths clean up: a failed dial must not leave the page's bytes in the cache directory.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&directory);
    Ok(serde_json::json!({"transferId": outcome?.to_string()}))
}

/// The state of a transfer this shell still has a row for.
pub(crate) fn transfer_status(
    state: &AppState,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    use synra_protocol::ids::TransferId;

    let id_text = string_arg(args, "transferId")?;
    let id = uuid::Uuid::parse_str(id_text)
        .map(TransferId::from_uuid)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"field": "transferId", "reason": error.to_string()}),
            )
        })?;
    let Some(snapshot) = state
        .transfers
        .list()
        .into_iter()
        .find(|transfer| transfer.transfer_id == id)
    else {
        // A completed transfer is removed from the registry by `finish_transfer`; the façade keeps the final state of
        // the transfers it started, so an unknown id here is usually "it is over" rather than "it never existed".
        return Err(
            IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "transferId": id_text,
                "reason": "no transfer by that id is in flight",
            })),
        );
    };
    Ok(serde_json::json!({
        "transferId": snapshot.transfer_id.to_string(),
        "state": transfer_state_name(&snapshot.state),
        "receivedBytes": snapshot.received_bytes,
        "totalBytes": snapshot.total_bytes,
        "blocksDone": snapshot.blocks_done,
        "blocksTotal": snapshot.blocks_total,
    }))
}
