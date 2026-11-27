use std::collections::BTreeSet;

use synra_core::bus::Delivery;
use synra_ipc::{
    IpcError,
    event::{KernelEvent, PluginDevicesChanged, PluginEvent},
};
use synra_plugin_ui::ValidatedSession;
use synra_protocol::TransferId;

use super::super::{authority, device_targets};
use super::{AppState, selectors::Topic, streams::Stream};
use crate::commands::transfers::unix_millis;

fn devices(state: &AppState) -> Result<PluginEvent, IpcError> {
    device_targets(state)
        .map(|devices| PluginEvent::DevicesChanged(PluginDevicesChanged { devices }))
}

pub(super) fn snapshots(
    state: &AppState,
    stream: &Stream,
    session: &ValidatedSession,
) -> Result<Vec<PluginEvent>, IpcError> {
    let mut events = Vec::new();
    if stream.observes(Topic::Devices) && Topic::Devices.allowed(session) {
        events.push(devices(state)?);
    }
    if stream.observes(Topic::Transfers) && Topic::Transfers.allowed(session) {
        events.extend(
            state
                .transfers
                .plugin_statuses(session.plugin_id(), unix_millis(), |id| {
                    stream.selects_transfer(id)
                })
                .into_iter()
                .map(PluginEvent::TransferProgress),
        );
    }
    Ok(events)
}

/// Resolve the management bus against current authoritative state and each session's selectors.
pub(crate) fn project_events(state: &AppState, delivery: &Delivery<KernelEvent>, at: i64) {
    let changes = Changes::of(delivery);
    if !changes.missed && !changes.devices && changes.transfers.is_empty() {
        return;
    }
    let _projection = state.plugin_events.projection.lock();
    let mut projected_devices = None;
    for (nonce, stream) in state.plugin_events.open() {
        let watches_devices =
            (changes.missed || changes.devices) && stream.observes(Topic::Devices);
        let watches_transfers =
            (changes.missed || !changes.transfers.is_empty()) && stream.observes(Topic::Transfers);
        if !watches_devices && !watches_transfers {
            continue;
        }
        let entry = state
            .hosted_ui
            .lock()
            .values()
            .find(|entry| entry.nonce == nonce)
            .cloned();
        let Some(entry) = entry else {
            state.plugin_events.close(nonce);
            continue;
        };
        let Ok(session) = authority::validate(state, &entry) else {
            continue;
        };
        let observe_devices = watches_devices && Topic::Devices.allowed(&session);
        let observe_transfers = watches_transfers && Topic::Transfers.allowed(&session);
        if changes.missed && (observe_devices || observe_transfers) {
            stream.invalidate();
            continue;
        }
        if observe_devices {
            let event = projected_devices.get_or_insert_with(|| devices(state));
            stream.publish(Topic::Devices, at, event.clone());
        }
        if observe_transfers {
            let now = unix_millis();
            for id in changes
                .transfers
                .iter()
                .copied()
                .filter(|id| stream.selects_transfer(*id))
            {
                if let Some(status) = state.transfers.plugin_status(session.plugin_id(), id, now) {
                    stream.publish(
                        Topic::Transfers,
                        at,
                        Ok(PluginEvent::TransferProgress(status)),
                    );
                }
            }
        }
    }
}

struct Changes {
    devices: bool,
    transfers: BTreeSet<TransferId>,
    missed: bool,
}

impl Changes {
    fn of(delivery: &Delivery<KernelEvent>) -> Self {
        let mut changes = Self {
            devices: false,
            transfers: BTreeSet::new(),
            missed: delivery.missed() != 0,
        };
        for event in delivery.events() {
            match event {
                KernelEvent::DeviceCardUpdated(_) | KernelEvent::DeviceRemoved { .. } => {
                    changes.devices = true
                }
                KernelEvent::TransferUpdated(row) => {
                    changes.transfers.insert(row.transfer_id);
                }
                KernelEvent::TransferFinished { transfer_id } => {
                    changes.transfers.insert(*transfer_id);
                }
                _ => {}
            }
        }
        changes
    }
}
