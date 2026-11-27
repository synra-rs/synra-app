use super::*;
use synra_core::transfer::TransferOutcome;

/// Closes a receive row whose transfer ended without a file.
///
/// The row was opened by the progress reporter when the proposal was accepted, and a sender that aborts never produces a
/// `FinishedTransfer` — so this is the only place that can tell the panel the row is over. It is not `completed`: nothing
/// arrived.
pub fn withdraw_received(state: &crate::state::AppState, transfer_id: TransferId) {
    if state.transfers.take_received(transfer_id).is_some() {
        state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
    }
}

/// Announces a received transfer as completed and retires its row.
///
/// The listener retires the receive row after confirming the object.
pub fn complete_received(
    state: &crate::state::AppState,
    transfer_id: synra_protocol::ids::TransferId,
) {
    if let Some(mut snapshot) = state.transfers.take_received(transfer_id) {
        snapshot.state = "completed".to_owned();
        snapshot.received_bytes = snapshot.total_bytes;
        snapshot.blocks_done = snapshot.blocks_total;
        state.publish(synra_ipc::event::KernelEvent::TransferUpdated(Box::new(
            snapshot,
        )));
        state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
    }
}

/// Removes a transfer and tells the interface it is over.
///
/// One function because the two halves must not be separated: a row removed without the announcement leaves the panel
/// showing a transfer that no longer exists, and an announcement without the removal leaves one it can never clear.
pub(crate) fn finish_transfer(
    state: &crate::state::AppState,
    transfer_id: synra_protocol::ids::TransferId,
    outcome: TransferOutcome,
) {
    let Some(temp) = state.transfers.finish(transfer_id, outcome, unix_millis()) else {
        return;
    };
    if let Some(path) = temp {
        let _ = std::fs::remove_file(path);
    }
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

pub use crate::commands::peers::address_of;
