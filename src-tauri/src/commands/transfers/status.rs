use std::collections::VecDeque;

use synra_core::transfer::{
    AcknowledgedProgress, TransferFailure, TransferLifecycle, TransferOutcome, TransferState,
};
use synra_ipc::dto::{
    PluginTransferFailure, PluginTransferState, PluginTransferStatus, TransferSnapshot,
};
use synra_protocol::ids::{PluginId, TransferId};

const RETAINED_TRANSFER_LIMIT: usize = 512;
const RETAINED_TRANSFER_MS: u64 = 10 * 60 * 1000;

#[derive(Debug)]
pub(super) struct OwnedStatus {
    pub(super) plugin: PluginId,
    pub(super) lifecycle: TransferLifecycle,
}

impl OwnedStatus {
    pub(super) fn new(plugin: PluginId, row: &TransferSnapshot) -> Self {
        Self {
            plugin,
            lifecycle: TransferLifecycle::new(AcknowledgedProgress {
                bytes: row.total_bytes,
                blocks: row.blocks_total,
            }),
        }
    }

    pub(super) fn snapshot(&self, transfer_id: TransferId) -> PluginTransferStatus {
        let (state, ended_at_ms, failure) = match self.lifecycle.state() {
            TransferState::Queued => (PluginTransferState::Pending, None, None),
            TransferState::Transferring => (PluginTransferState::Transferring, None, None),
            TransferState::Finished { outcome, at_ms } => match outcome {
                TransferOutcome::Completed => (PluginTransferState::Completed, Some(at_ms), None),
                TransferOutcome::Cancelled => (PluginTransferState::Cancelled, Some(at_ms), None),
                TransferOutcome::Failed(failure) => (
                    PluginTransferState::Failed,
                    Some(at_ms),
                    Some(project_failure(failure)),
                ),
            },
        };
        let acknowledged = self.lifecycle.progress();
        let total = self.lifecycle.total();
        PluginTransferStatus {
            transfer_id,
            state,
            received_bytes: acknowledged.bytes,
            total_bytes: total.bytes,
            blocks_done: acknowledged.blocks,
            blocks_total: total.blocks,
            revision: self.lifecycle.revision(),
            ended_at_ms,
            failure,
        }
    }
}

fn project_failure(failure: TransferFailure) -> PluginTransferFailure {
    match failure {
        TransferFailure::Connection => PluginTransferFailure::Connection,
        TransferFailure::TimedOut => PluginTransferFailure::TimedOut,
        TransferFailure::Authorization => PluginTransferFailure::Authorization,
        TransferFailure::PeerAborted(reason) => PluginTransferFailure::PeerAborted { reason },
        TransferFailure::FileIo => PluginTransferFailure::FileIo,
        TransferFailure::Integrity => PluginTransferFailure::Integrity,
        TransferFailure::Protocol => PluginTransferFailure::Protocol,
        TransferFailure::WorkerUnavailable => PluginTransferFailure::WorkerUnavailable,
    }
}

#[derive(Debug, Default)]
pub(super) struct TerminalStatuses(VecDeque<(TransferId, OwnedStatus)>);

impl TerminalStatuses {
    pub(super) fn add(&mut self, id: TransferId, status: OwnedStatus, now_ms: u64) {
        self.0.retain(|(_, record)| is_retained(record, now_ms));
        if self.0.len() >= RETAINED_TRANSFER_LIMIT {
            self.0.pop_front();
        }
        self.0.push_back((id, status));
    }

    pub(super) fn get(&self, id: TransferId, now_ms: u64) -> Option<&OwnedStatus> {
        self.0
            .iter()
            .find_map(|(key, record)| (*key == id && is_retained(record, now_ms)).then_some(record))
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(super) fn retained(&self, now_ms: u64) -> impl Iterator<Item = (TransferId, &OwnedStatus)> {
        self.0
            .iter()
            .filter_map(move |(id, record)| is_retained(record, now_ms).then_some((*id, record)))
    }
}

fn is_retained(status: &OwnedStatus, now_ms: u64) -> bool {
    matches!(status.lifecycle.state(), TransferState::Finished { at_ms, .. } if now_ms.saturating_sub(at_ms) < RETAINED_TRANSFER_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_records_expire_and_evict_the_oldest_at_capacity() {
        let mut records = TerminalStatuses::default();
        let owner = PluginId::parse("@acme/sender").unwrap();
        let first = TransferId::new();
        let mut status = OwnedStatus {
            plugin: owner.clone(),
            lifecycle: TransferLifecycle::new(AcknowledgedProgress {
                bytes: 1,
                blocks: 1,
            }),
        };
        assert!(status.lifecycle.finish(TransferOutcome::Cancelled, 7));
        records.add(first, status, 7);
        assert!(records.get(first, RETAINED_TRANSFER_MS + 6).is_some());
        assert!(records.get(first, RETAINED_TRANSFER_MS + 7).is_none());

        for _ in 0..RETAINED_TRANSFER_LIMIT {
            let mut status = OwnedStatus {
                plugin: owner.clone(),
                lifecycle: TransferLifecycle::new(AcknowledgedProgress {
                    bytes: 1,
                    blocks: 1,
                }),
            };
            assert!(status.lifecycle.finish(TransferOutcome::Cancelled, 8));
            records.add(TransferId::new(), status, 8);
        }
        assert_eq!(records.0.len(), RETAINED_TRANSFER_LIMIT);
        assert!(records.get(first, 8).is_none());
    }
}
