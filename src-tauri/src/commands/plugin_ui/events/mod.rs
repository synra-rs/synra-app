//! Session RPC and authorization for the plugin event projection.

mod projection;
mod selectors;
mod streams;

use crate::{AppState, state::HostedUi};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use synra_ipc::{IpcError, PluginUiOp};
use synra_plugin_ui::ValidatedSession;
use synra_protocol::{ErrorCode, TransferId, inbound::HandlePattern};
use uuid::Uuid;

use super::{authority, internal, transfer::owned_status};
pub(crate) use projection::bind_custom_events;
pub(crate) use projection::project_events;
use selectors::Selector;
pub(crate) use streams::PluginEvents;

const POLL_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(untagged)]
enum Subscribe {
    Pattern(PatternSubscription),
    Transfer(TransferSubscription),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatternSubscription {
    subscription_id: u32,
    pattern: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferSubscription {
    subscription_id: u32,
    transfer_id: TransferId,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recover {
    stream_id: String,
    after_seq: u64,
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
            let (subscription_id, selector) = match args {
                Subscribe::Pattern(args) => (
                    args.subscription_id,
                    Selector::Pattern(HandlePattern::parse(&args.pattern).map_err(invalid)?),
                ),
                Subscribe::Transfer(args) => {
                    (args.subscription_id, Selector::Transfer(args.transfer_id))
                }
            };
            selector.allows(&session)?;
            if let Selector::Transfer(id) = &selector {
                owned_status(state, &session, *id)?;
            }
            let _projection = state.plugin_events.projection.lock();
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
                .subscribe(nonce, subscription_id, selector)?;
            drop(hosted);
            Ok(Value::String(stream.to_string()))
        }
        PluginUiOp::EventUnsubscribe => {
            let args: Subscription = serde_json::from_value(args.clone()).map_err(invalid)?;
            let stream_id = args.stream_id.parse::<Uuid>().map_err(invalid)?;
            let _projection = state.plugin_events.projection.lock();
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
            let args: Recover = serde_json::from_value(args.clone()).map_err(invalid)?;
            let id = args.stream_id.parse::<Uuid>().map_err(invalid)?;
            let stream = state
                .plugin_events
                .stream(nonce, id)
                .ok_or_else(|| invalid("the event stream has been closed"))?;
            let _projection = state.plugin_events.projection.lock();
            let snapshots = projection::snapshots(state, &stream, &session)?;
            validate_page(state, label, nonce)?;
            serde_json::to_value(stream.reset(args.after_seq, snapshots)?).map_err(internal)
        }
        _ => Err(IpcError::unknown(ErrorCode::Unsupported)),
    }
}

#[cfg(test)]
mod tests;
