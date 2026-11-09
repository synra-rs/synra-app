use synra_ipc::IpcError;

/// Turns a storage error into the refusal a command returns.
///
/// Named rather than written inline because it appears in every command that reads the database, and the one thing all
/// of them must agree on is that a storage failure is `internal` and not `invalid_argument`: a user who sent a valid
/// request does not need to be told they sent it wrong.
pub(crate) fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(synra_protocol::ErrorCode::Internal)
        .with_params(serde_json::json!({"detail": error.to_string()}))
}
