use super::super::*;
use super::{MAX_PLUGIN_TRANSFER_BYTES, transfer_file_name};
use crate::commands::transfers;

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

/// Reads the current or retained state of a transfer this plugin started.
pub(crate) fn transfer_status(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    use synra_protocol::ids::TransferId;

    require_grant(session, "file:transfer:send", "transfer.status")?;
    let id_text = string_arg(args, "transferId")?;
    let id = uuid::Uuid::parse_str(id_text)
        .map(TransferId::from_uuid)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"field": "transferId", "reason": error.to_string()}),
            )
        })?;
    let snapshot = state
        .transfers
        .plugin_status(session.plugin_id(), id, transfers::unix_millis())
        .ok_or_else(|| {
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "transferId": id,
                "reason": "transfer_not_found",
            }))
        })?;
    serde_json::to_value(snapshot).map_err(internal)
}
