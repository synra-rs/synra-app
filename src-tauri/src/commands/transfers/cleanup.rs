use super::*;

/// Closes a receive row whose transfer ended without a file.
///
/// The row was opened by the progress reporter when the proposal was accepted, and a sender that aborts never produces a
/// `FinishedTransfer` — so this is the only place that can tell the panel the row is over. It is not `completed`: nothing
/// arrived.
pub fn withdraw_received(state: &crate::state::AppState, transfer_id: TransferId) {
    state.transfers.forget(&transfer_id);
    state.publish(synra_ipc::event::KernelEvent::TransferFinished { transfer_id });
}

/// Announces a received transfer as completed and retires its row.
///
/// The receiving half of a transfer has no worker to remove the row; without this the panel would show it as arriving
/// for ever. A transfer that never registered a row is still finished: the announcement is how the panel learns the id
/// is done.
pub fn complete_received(
    state: &crate::state::AppState,
    transfer_id: synra_protocol::ids::TransferId,
) {
    // The registry may not have the row — a transfer whose first blocks arrived before the reporter attached, or one
    // whose connection ended badly. Finishing it is still right: the announcement is how the panel learns the id is done.
    if let Some(mut snapshot) = state
        .transfers
        .list()
        .into_iter()
        .find(|row| row.transfer_id == transfer_id)
    {
        snapshot.state = "completed".to_owned();
        snapshot.received_bytes = snapshot.total_bytes;
        snapshot.blocks_done = snapshot.blocks_total;
        announce_transfer(state, snapshot);
    }
    finish_transfer(state, transfer_id);
}

/// Removes a transfer and tells the interface it is over.
///
/// One function because the two halves must not be separated: a row removed without the announcement leaves the panel
/// showing a transfer that no longer exists, and an announcement without the removal leaves one it can never clear.
pub(crate) fn finish_transfer(
    state: &crate::state::AppState,
    transfer_id: synra_protocol::ids::TransferId,
) {
    let (temp, directory) = state.transfers.owned_files(&transfer_id);
    if let Some(path) = temp {
        let _ = std::fs::remove_file(path);
    }
    if let Some(directory) = directory {
        // Nonrecursive: ownership of one upload directory never permits deleting unrelated contents.
        let _ = std::fs::remove_dir(directory);
    }
    state.transfers.forget(&transfer_id);
    state.transfers.untrack(&transfer_id);
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
