use super::super::*;
use crate::commands::transfers;

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
