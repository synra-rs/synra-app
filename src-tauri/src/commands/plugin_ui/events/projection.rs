use std::collections::BTreeSet;
use std::sync::Arc;

use synra_core::bus::Delivery;
use synra_ipc::{
    IpcError,
    event::{KernelEvent, PluginCustomEvent, PluginDevicesChanged, PluginEvent},
};
use synra_plugin_ui::ValidatedSession;
use synra_protocol::TransferId;

use super::super::{authority, device_targets, require_grant};
use super::{
    AppState,
    selectors::{Topic, custom_grant},
    streams::Stream,
};
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
        let Some(session) = page_session(state, nonce) else {
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
            stream.publish(at, event.clone().map_err(|error| (Topic::Devices, error)));
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
                    stream.publish(at, Ok(PluginEvent::TransferProgress(status)));
                }
            }
        }
    }
}

fn page_session(state: &AppState, nonce: u128) -> Option<ValidatedSession> {
    let entry = state
        .hosted_ui
        .lock()
        .values()
        .find(|entry| entry.nonce == nonce)
        .cloned();
    if let Some(entry) = entry {
        authority::validate(state, &entry).ok()
    } else {
        state.plugin_events.close(nonce);
        None
    }
}

/// Wire the runtime observer to the page projection without keeping the application alive.
pub(crate) fn bind_custom_events(state: &Arc<AppState>) {
    let service = state
        .plugins
        .as_ref()
        .and_then(|plugins| plugins.lock().event_service());
    if let Some(service) = service {
        let weak = Arc::downgrade(state);
        service.set_observer(Arc::new(move |name, payload| {
            let Some(state) = weak.upgrade() else { return };
            if let Some(event) = PluginCustomEvent::new(name.clone(), payload.clone()) {
                project_custom(
                    &state,
                    PluginEvent::Custom(event),
                    synra_store::sqlite::now_ms(),
                );
            }
        }));
    }
}

pub(super) fn project_custom(state: &AppState, event: PluginEvent, at: i64) {
    let grant = custom_grant(event.name());
    let _projection = state.plugin_events.projection.lock();
    for (nonce, stream) in state.plugin_events.open() {
        if stream.selects(&event)
            && let Some(session) = page_session(state, nonce)
            && require_grant(&session, &grant, "event.poll").is_ok()
        {
            stream.publish(at, Ok(event.clone()));
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
