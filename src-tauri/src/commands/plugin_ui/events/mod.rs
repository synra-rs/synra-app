//! Session RPC and authorization for the plugin event projection.

mod streams;

use crate::{AppState, state::HostedUi};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use synra_core::bus::Delivery;
use synra_ipc::event::{KernelEvent, PluginDevicesChanged, PluginEvent};
use synra_ipc::{IpcError, PluginUiOp};
use synra_plugin_ui::ValidatedSession;
use synra_protocol::{ErrorCode, inbound::HandlePattern};
use uuid::Uuid;

use super::{authority, device_targets, internal, require_grant};
pub(crate) use streams::PluginEvents;

const DEVICE_GRANT: &str = "device:list";
const POLL_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Subscribe {
    subscription_id: u32,
    pattern: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Subscription {
    subscription_id: u32,
    stream_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receive {
    stream_id: String,
}

fn invalid(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::InvalidArgument)
        .with_params(serde_json::json!({"reason": error.to_string()}))
}

fn validate_page(
    state: &AppState,
    label: &str,
    nonce: u128,
) -> Result<(HostedUi, ValidatedSession), IpcError> {
    let entry = state
        .hosted_ui
        .lock()
        .get(label)
        .filter(|entry| entry.nonce == nonce)
        .cloned()
        .ok_or_else(authority::refused)?;
    let session = authority::validate(state, &entry)?;
    Ok((entry, session))
}

pub(crate) async fn dispatch_event_rpc(
    state: &AppState,
    label: &str,
    token: &str,
    method: &str,
    args: &Value,
) -> Option<Result<Value, IpcError>> {
    let operation = PluginUiOp::parse(method)?;
    if !matches!(
        operation,
        PluginUiOp::EventSubscribe
            | PluginUiOp::EventUnsubscribe
            | PluginUiOp::EventPoll
            | PluginUiOp::EventResync
    ) {
        return None;
    }
    Some(event_rpc(state, label, token, operation, args).await)
}

async fn event_rpc(
    state: &AppState,
    label: &str,
    token: &str,
    op: PluginUiOp,
    args: &Value,
) -> Result<Value, IpcError> {
    let nonce = token.parse().map_err(|_| invalid("invalid page token"))?;
    let (entry, session) = validate_page(state, label, nonce)?;
    match op {
        PluginUiOp::EventSubscribe => {
            let args: Subscribe = serde_json::from_value(args.clone()).map_err(invalid)?;
            let pattern = HandlePattern::parse(&args.pattern).map_err(invalid)?;
            if pattern.matches(PluginEvent::DEVICES_CHANGED) {
                require_grant(&session, DEVICE_GRANT, op.as_str())?;
            }
            // Keep the reservation alive across registration, so closing this window cannot leave a new stream behind.
            let hosted = state.hosted_ui.lock();
            if !hosted
                .get(label)
                .is_some_and(|current| current.nonce == entry.nonce)
            {
                return Err(authority::refused());
            }
            let stream = state
                .plugin_events
                .subscribe(nonce, args.subscription_id, pattern)?;
            Ok(Value::String(stream.to_string()))
        }
        PluginUiOp::EventUnsubscribe => {
            let args: Subscription = serde_json::from_value(args.clone()).map_err(invalid)?;
            let stream_id = args.stream_id.parse::<Uuid>().map_err(invalid)?;
            state
                .plugin_events
                .unsubscribe(nonce, stream_id, args.subscription_id);
            Ok(Value::Null)
        }
        PluginUiOp::EventPoll => {
            let args: Receive = serde_json::from_value(args.clone()).map_err(invalid)?;
            let id = args.stream_id.parse::<Uuid>().map_err(invalid)?;
            let stream = state
                .plugin_events
                .stream(nonce, id)
                .ok_or_else(|| invalid("the event stream has been closed"))?;
            let message = stream.poll(POLL_DEADLINE).await;
            validate_page(state, label, nonce)?;
            serde_json::to_value(message?).map_err(internal)
        }
        PluginUiOp::EventResync => {
            let args: Receive = serde_json::from_value(args.clone()).map_err(invalid)?;
            let id = args.stream_id.parse::<Uuid>().map_err(invalid)?;
            let stream = state
                .plugin_events
                .stream(nonce, id)
                .ok_or_else(|| invalid("the event stream has been closed"))?;
            let _projection = state.plugin_events.projection.lock();
            if !stream.matches(PluginEvent::DEVICES_CHANGED) {
                return Ok(Value::Null);
            }
            require_grant(&session, DEVICE_GRANT, op.as_str())?;
            let devices = device_targets(state)?;
            validate_page(state, label, nonce)?;
            let event = PluginEvent::DevicesChanged(PluginDevicesChanged { devices });
            serde_json::to_value(stream.reset(synra_store::sqlite::now_ms(), event))
                .map_err(internal)
        }
        _ => Err(IpcError::unknown(ErrorCode::Unsupported)),
    }
}

/// Project one coherent paired-device snapshot after a change or a gap in the host bus.
pub(crate) fn project_events(state: &AppState, delivery: &Delivery<KernelEvent>, at: i64) {
    if delivery.missed() == 0
        && !delivery.events().iter().any(|event| {
            matches!(
                event,
                KernelEvent::DeviceCardUpdated(_) | KernelEvent::DeviceRemoved { .. }
            )
        })
    {
        return;
    }
    let topic = PluginEvent::DEVICES_CHANGED;
    let candidates = state.plugin_events.interested(topic);
    if candidates.is_empty() {
        return;
    }
    let _projection = state.plugin_events.projection.lock();
    let projected = device_targets(state)
        .map(|devices| PluginEvent::DevicesChanged(PluginDevicesChanged { devices }));
    for (nonce, stream) in candidates {
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
        if authority::validate(state, &entry).is_ok_and(|session| {
            require_grant(&session, DEVICE_GRANT, PluginUiOp::EventPoll.as_str()).is_ok()
        }) {
            stream.publish(topic, at, &projected);
        }
    }
}

#[cfg(test)]
mod tests;
