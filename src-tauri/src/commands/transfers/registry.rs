use super::*;

/// The transfers in flight, by identifier.
#[derive(Debug, Default)]
pub struct Transfers {
    /// Keyed by the transfer, not ordered: `list` sorts, so the worker never has to.
    by_id: Mutex<HashMap<TransferId, Entry>>,
    /// The token that stops each worker, when a transfer has one.
    ///
    /// Beside the snapshots rather than inside them: a token is not part of what a panel draws, and a `TransferSnapshot`
    /// that carried one would have to be `Clone` over a handle or lose it in the clone.
    cancels: Mutex<HashMap<TransferId, synra_core::supervisor::Cancellation>>,
}

/// One transfer: what the interface shows, and what this shell owns on disk.
#[derive(Debug, Clone)]
struct Entry {
    /// What the panel draws.
    snapshot: TransferSnapshot,
    /// A file this shell created for the transfer, to be removed when the transfer ends.
    ///
    /// Carried here rather than recomputed from the snapshot because the path is not derivable: a sent file belongs to the user
    /// and must **not** be deleted, while a sent message was written by this shell and must be. The two look identical from the
    /// snapshot alone.
    temp: Option<std::path::PathBuf>,
    /// Upload directory owned by this task; never inferred from a user's source file.
    cleanup_dir: Option<std::path::PathBuf>,
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
    pub fn record(&self, snapshot: TransferSnapshot) {
        let mut registry = self.by_id.lock();
        if let Some(entry) = registry.get_mut(&snapshot.transfer_id) {
            entry.snapshot = snapshot;
        } else {
            registry.insert(
                snapshot.transfer_id,
                Entry {
                    snapshot,
                    temp: None,
                    cleanup_dir: None,
                },
            );
        }
    }

    /// Records a snapshot and the file this shell created for it.
    pub fn record_with_temp(&self, snapshot: TransferSnapshot, temp: std::path::PathBuf) {
        self.by_id.lock().insert(
            snapshot.transfer_id,
            Entry {
                snapshot,
                temp: Some(temp),
                cleanup_dir: None,
            },
        );
    }

    /// Registers all task-owned artifacts before the worker can finish or fail to start.
    pub(super) fn record_owned(
        &self,
        snapshot: TransferSnapshot,
        temp: Option<std::path::PathBuf>,
        cleanup_dir: Option<std::path::PathBuf>,
    ) {
        self.by_id.lock().insert(
            snapshot.transfer_id,
            Entry {
                snapshot,
                temp,
                cleanup_dir,
            },
        );
    }

    /// Copies cleanup ownership without withdrawing the row before filesystem work has finished.
    pub(super) fn owned_files(
        &self,
        transfer_id: &TransferId,
    ) -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
        self.by_id
            .lock()
            .get(transfer_id)
            .map_or((None, None), |entry| {
                (entry.temp.clone(), entry.cleanup_dir.clone())
            })
    }

    /// Drops a transfer, answering the file the caller should delete.
    ///
    /// Returning the path rather than deleting it here keeps this type free of the filesystem: the registry's job is to know
    /// what is in flight, and a caller that cancels is the one that knows whether the transfer was a message of its own.
    pub fn forget(&self, transfer_id: &TransferId) -> Option<std::path::PathBuf> {
        self.by_id
            .lock()
            .remove(transfer_id)
            .and_then(|entry| entry.temp)
    }

    /// Remembers the token that stops the worker for this transfer.
    pub fn track_cancel(
        &self,
        transfer_id: TransferId,
        cancel: synra_core::supervisor::Cancellation,
    ) {
        self.cancels.lock().insert(transfer_id, cancel);
    }

    /// Takes the token out and asks it to stop.
    ///
    /// Answers whether a worker was there to ask. A token is removed on the way out: a cancel arriving after the worker
    /// finished must not leave one behind for a later transfer that reuses the identifier (identifiers are v4 and do not
    /// repeat, but a map that only grows is a leak either way).
    pub fn stop(&self, transfer_id: &TransferId) -> bool {
        match self.cancels.lock().remove(transfer_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Forgets a token whose worker has finished on its own.
    pub fn untrack(&self, transfer_id: &TransferId) {
        self.cancels.lock().remove(transfer_id);
    }

    /// Every transfer, ordered by identifier.
    ///
    /// Ordered by identifier rather than by start time because the list is read repeatedly: sorting by arrival would let two
    /// transfers swap places between two reads, which the interface would draw as two rows moving for no reason.
    #[must_use]
    pub fn list(&self) -> Vec<TransferSnapshot> {
        let mut transfers: Vec<TransferSnapshot> = self
            .by_id
            .lock()
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect();
        transfers.sort_by_key(|transfer| transfer.transfer_id);
        transfers
    }

    /// How many are in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.lock().len()
    }

    /// Whether nothing is in flight.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.lock().is_empty()
    }
}
