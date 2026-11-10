//! What the transfer registry does with what a worker reports.

use super::Transfers;
use synra_ipc::dto::TransferSnapshot;
use synra_protocol::ids::{DeviceId, TransferId};

mod consent;

/// A snapshot a worker might report, with every field named.
fn snapshot(transfer_id: TransferId, state: &str) -> TransferSnapshot {
    TransferSnapshot {
        transfer_id,
        peer_device: DeviceId::new(),
        peer_name: "Study PC".to_owned(),
        direction: "send".to_owned(),
        kind: "plugin-bundle".to_owned(),
        state: state.to_owned(),
        file_name: "bundle.zip".to_owned(),
        received_bytes: 0,
        total_bytes: 1_024,
        blocks_done: 0,
        blocks_total: 1,
        abort_reason: None,
        started_at_ms: 1_000,
    }
}

/// A progress step, as the listener reports it.
fn progress_step(
    transfer_id: TransferId,
    bundle: Option<&str>,
    received: u64,
    blocks_done: u32,
) -> synra_core::listener::InboundProgressEvent {
    synra_core::listener::InboundProgressEvent {
        peer: DeviceId::new(),
        progress: synra_core::session::InboundProgress {
            transfer_id,
            file_name: "photo.png".to_owned(),
            bundle: bundle.map(|id| synra_protocol::ids::PluginId::parse(id).expect("a plugin id")),
            received_bytes: received,
            total_bytes: 2_048,
            blocks_done,
            blocks_total: 4,
        },
        aborted: false,
    }
}

mod cancellation;
mod consent_policy;
mod inbox;
mod outbound;
mod receive;
mod send;
mod snapshots;
