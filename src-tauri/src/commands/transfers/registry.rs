use super::status::{OwnedStatus, TerminalStatuses};
use super::*;
use synra_core::supervisor::Cancellation;
use synra_core::transfer::{AcknowledgedProgress, TransferOutcome};
use synra_ipc::dto::PluginTransferStatus;
use synra_protocol::ids::PluginId;

/// The transfers in flight, by identifier.
#[derive(Debug, Default)]
pub struct Transfers {
    /// Keyed by the transfer, not ordered: `list` sorts, so the worker never has to.
    registry: Mutex<Registry>,
}

pub(super) enum CancelTarget {
    Worker(Cancellation),
    Listener,
}

#[derive(Debug, Default)]
struct Registry {
    active: HashMap<TransferId, Entry>,
    terminal: TerminalStatuses,
}

/// One transfer: what the interface shows, and what this shell owns on disk.
#[derive(Debug)]
struct Entry {
    /// What the panel draws.
    snapshot: TransferSnapshot,
    /// A file this shell created for the transfer, to be removed when the transfer ends.
    ///
    /// Carried here rather than recomputed from the snapshot because the path is not derivable: a sent file belongs to the user
    /// and must **not** be deleted, while a sent message was written by this shell and must be. The two look identical from the
    /// snapshot alone.
    temp: Option<std::path::PathBuf>,
    status: Option<OwnedStatus>,
    cancel: Option<Cancellation>,
}

impl Transfers {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what the worker just reported, replacing the previous snapshot of the same transfer.
    ///
    /// Replacement rather than merge: a snapshot is a complete answer, and merging two would let a field the worker stopped
    /// reporting keep its old value for ever — a progress bar that freezes while the transfer moves.
    ///
    /// The temporary file is **kept**: this is called on every block, and a version that cleared it would forget what to delete
    /// on the second report — which is the one moment a cancel is likely to arrive.
    pub fn record(&self, snapshot: TransferSnapshot) -> bool {
        let mut registry = self.registry.lock();
        if let Some(entry) = registry.active.get_mut(&snapshot.transfer_id) {
            if entry.snapshot.peer_device != snapshot.peer_device
                || entry.snapshot.direction != snapshot.direction
            {
                return false;
            }
            if let Some(status) = &mut entry.status {
                status.lifecycle.acknowledge(AcknowledgedProgress {
                    bytes: snapshot.received_bytes,
                    blocks: snapshot.blocks_done,
                });
            }
            entry.snapshot = snapshot;
            return true;
        }
        if registry
            .terminal
            .get(snapshot.transfer_id, unix_millis())
            .is_some()
        {
            return false;
        }
        registry.active.insert(
            snapshot.transfer_id,
            Entry {
                snapshot,
                temp: None,
                status: None,
                cancel: None,
            },
        );
        true
    }

    /// Records a snapshot and the file this shell created for it.
    pub fn record_with_temp(&self, snapshot: TransferSnapshot, temp: std::path::PathBuf) {
        self.registry.lock().active.insert(
            snapshot.transfer_id,
            Entry {
                snapshot,
                temp: Some(temp),
                status: None,
                cancel: None,
            },
        );
    }

    /// Registers all task-owned artifacts before the worker can finish or fail to start.
    pub(super) fn record_owned(
        &self,
        snapshot: TransferSnapshot,
        temp: Option<std::path::PathBuf>,
        plugin: Option<PluginId>,
        cancel: Cancellation,
    ) {
        let status = plugin.map(|owner| OwnedStatus::new(owner, &snapshot));
        self.registry.lock().active.insert(
            snapshot.transfer_id,
            Entry {
                snapshot,
                temp,
                status,
                cancel: Some(cancel),
            },
        );
    }

    pub(super) fn start_transferring(&self, id: TransferId) {
        if let Some(status) = self
            .registry
            .lock()
            .active
            .get_mut(&id)
            .and_then(|entry| entry.status.as_mut())
        {
            status.lifecycle.start();
        }
    }

    /// Moves the record to terminal retention and takes exclusive cleanup ownership.
    pub(super) fn finish(
        &self,
        id: TransferId,
        outcome: TransferOutcome,
        now_ms: u64,
    ) -> Option<Option<std::path::PathBuf>> {
        let mut registry = self.registry.lock();
        let entry = registry.active.get_mut(&id)?;
        if entry.snapshot.direction != "send"
            || entry
                .status
                .as_mut()
                .is_some_and(|status| !status.lifecycle.finish(outcome, now_ms))
        {
            return None;
        }
        let entry = registry.active.remove(&id)?;
        if let Some(status) = entry.status {
            registry.terminal.add(id, status, now_ms);
        }
        Some(entry.temp)
    }

    pub(crate) fn plugin_status(
        &self,
        owner: &PluginId,
        id: TransferId,
        now_ms: u64,
    ) -> Option<PluginTransferStatus> {
        let registry = self.registry.lock();
        let status = registry
            .active
            .get(&id)
            .and_then(|entry| entry.status.as_ref())
            .or_else(|| registry.terminal.get(id, now_ms))?;
        (status.plugin == *owner).then(|| status.snapshot(id))
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(crate) fn plugin_statuses(
        &self,
        owner: &PluginId,
        now_ms: u64,
        selected: impl Fn(TransferId) -> bool,
    ) -> Vec<PluginTransferStatus> {
        let registry = self.registry.lock();
        let active = registry
            .active
            .iter()
            .filter_map(|(id, entry)| entry.status.as_ref().map(|status| (*id, status)));
        let mut snapshots: Vec<_> = active
            .chain(registry.terminal.retained(now_ms))
            .filter_map(|(id, status)| {
                (status.plugin == *owner && selected(id)).then(|| status.snapshot(id))
            })
            .collect();
        drop(registry);
        snapshots.sort_unstable_by_key(|status| status.transfer_id);
        snapshots
    }

    /// Drops a transfer, answering the file the caller should delete.
    ///
    /// Returning the path rather than deleting it here keeps this type free of the filesystem: the registry's job is to know
    /// what is in flight, and a caller that cancels is the one that knows whether the transfer was a message of its own.
    #[cfg(test)]
    pub(super) fn forget(&self, transfer_id: &TransferId) -> Option<std::path::PathBuf> {
        let mut registry = self.registry.lock();
        if registry.active.get(transfer_id)?.status.is_some() {
            return None;
        }
        registry
            .active
            .remove(transfer_id)
            .and_then(|entry| entry.temp)
    }

    pub(super) fn take_received(&self, id: TransferId) -> Option<TransferSnapshot> {
        let mut registry = self.registry.lock();
        if registry.active.get(&id)?.snapshot.direction != "receive" {
            return None;
        }
        registry.active.remove(&id).map(|entry| entry.snapshot)
    }

    pub(super) fn cancel_target(&self, id: TransferId) -> Option<CancelTarget> {
        let registry = self.registry.lock();
        let entry = registry.active.get(&id)?;
        if entry.snapshot.direction == "receive" {
            return Some(CancelTarget::Listener);
        }
        entry.cancel.clone().map(CancelTarget::Worker)
    }

    /// Every transfer, ordered by identifier.
    ///
    /// Ordered by identifier rather than by start time because the list is read repeatedly: sorting by arrival would let two
    /// transfers swap places between two reads, which the interface would draw as two rows moving for no reason.
    #[must_use]
    pub fn list(&self) -> Vec<TransferSnapshot> {
        let mut transfers: Vec<TransferSnapshot> = self
            .registry
            .lock()
            .active
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect();
        transfers.sort_by_key(|transfer| transfer.transfer_id);
        transfers
    }

    /// How many are in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registry.lock().active.len()
    }

    /// Whether nothing is in flight.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registry.lock().active.is_empty()
    }
}
