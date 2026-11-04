//! Host-only incoming pointer consent and stop commands.
use crate::state::{AppState, internal};
use synra_ipc::{InputSnapshot, IpcError};
use synra_protocol::ids::SessionId;

/// Incoming pointer consent state for the host UI, never the plugin command surface.
pub fn devices_input_snapshot(state: &AppState) -> Result<InputSnapshot, IpcError> {
    Ok(state.input.snapshot())
}

/// Decide exactly the pending session the host displayed; stale decisions are refused.
pub fn devices_input_respond(
    state: &AppState,
    session_id: &str,
    allow: bool,
) -> Result<(), IpcError> {
    let id = session_id.parse::<SessionId>().map_err(internal)?;
    if state.input.receiver.decide(id, allow) {
        Ok(())
    } else {
        Err(internal("this input request is no longer pending"))
    }
}

/// Stop an accepted controller or withdraw a still-pending request.
pub fn devices_input_stop(state: &AppState, session_id: &str) -> Result<(), IpcError> {
    let id = session_id.parse::<SessionId>().map_err(internal)?;
    state.input.receiver.stop(id);
    Ok(())
}
