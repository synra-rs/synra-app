use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde_json::{Value, json};
use synra_capability::CapabilitySet;
use synra_ipc::event::{KernelEvent, PluginEvent, PluginEventEnvelope, PluginEventRecovery};
use synra_protocol::ids::PluginId;

use super::{AppState, ErrorCode, IpcError, dispatch_event_rpc, project_events};
use crate::state::HostedUi;

const PC: &str = "11111111-1111-4111-8111-111111111111";
const DEADLINE: Duration = Duration::from_secs(2);

fn page(state: &AppState, label: &str, nonce: u128, id: &str, grants: &[&str]) {
    state
        .db()
        .insert_plugin(id, true, true, "installed", 1, 1)
        .unwrap();
    open_page(state, label, nonce, id, grants);
}

fn open_page(state: &AppState, label: &str, nonce: u128, id: &str, grants: &[&str]) {
    let plugin_id = PluginId::parse(id).unwrap();
    let authority = state.db().plugin_authority(id).unwrap().unwrap();
    state.ui_sessions.issue_with(
        &plugin_id,
        "1.0.0",
        CapabilitySet::parse_all(grants.iter().copied()).unwrap(),
        nonce,
        synra_store::sqlite::now_ms() as u64,
    );
    state.hosted_ui.lock().insert(
        label.to_owned(),
        HostedUi {
            plugin_id,
            authority,
            version: "1.0.0".to_owned(),
            ready: true,
            nonce,
        },
    );
}

async fn rpc(
    state: &AppState,
    label: &str,
    nonce: u128,
    method: &str,
    args: Value,
) -> Result<Value, IpcError> {
    dispatch_event_rpc(state, label, &nonce.to_string(), method, &args)
        .await
        .unwrap()
}

async fn subscribe(state: &AppState, label: &str, nonce: u128, pattern: &str, id: u32) -> Value {
    rpc(
        state,
        label,
        nonce,
        "event.subscribe",
        json!({"pattern": pattern, "subscriptionId": id}),
    )
    .await
    .unwrap()
}

async fn pending<T>(mut future: Pin<&mut impl Future<Output = T>>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

fn device_change(state: &AppState, at: i64) {
    let listener = state.events.subscribe();
    state.publish(KernelEvent::DeviceRemoved {
        device_id: PC.parse().unwrap(),
    });
    let delivery = state.events.take(listener).unwrap();
    state.events.unsubscribe(listener);
    project_events(state, &delivery, at);
}

#[tokio::test]
async fn two_pages_receive_only_authorized_subscribed_projections() {
    let (_root, state) = crate::testing::file_state();
    page(&state, "reader", 10, "@test/reader", &["device:*"]);
    page(&state, "other", 11, "@test/other", &[]);
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: PC.to_owned(),
            display_name: "Laptop".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            address: Some("127.0.0.1:1234".to_owned()),
            paired_at_ms: crate::testing::FIXED_NOW_MS,
        })
        .unwrap();
    let stream = subscribe(&state, "reader", 10, "*.changed", 1).await;
    let unrelated = subscribe(&state, "other", 11, "custom.ready", 1).await;
    let denied = rpc(
        &state,
        "other",
        11,
        "event.subscribe",
        json!({"subscriptionId": 2, "pattern": "device.changed"}),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::CapabilityDenied);
    let denied = rpc(
        &state,
        "other",
        10,
        "event.poll",
        json!({"streamId": stream}),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::CapabilityDenied);
    let denied = rpc(
        &state,
        "other",
        11,
        "event.poll",
        json!({"streamId": stream}),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::InvalidArgument);

    let mut receive = Box::pin(rpc(
        &state,
        "reader",
        10,
        "event.poll",
        json!({"streamId": stream}),
    ));
    let mut other = Box::pin(rpc(
        &state,
        "other",
        11,
        "event.poll",
        json!({"streamId": unrelated}),
    ));
    pending(receive.as_mut()).await;
    pending(other.as_mut()).await;
    device_change(&state, 23);
    let frame = tokio::time::timeout(DEADLINE, receive)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        frame,
        json!({
            "seq": 1, "at": 23, "name": "device.changed",
            "payload": {"devices": [{"deviceId": PC, "displayName": "Laptop", "platform": "linux", "online": false}]}
        })
    );
    pending(other.as_mut()).await;
    state.ui_sessions.revoke(11);
    assert_eq!(super::super::stale_windows(&state), vec!["other"]);
    let refusal = tokio::time::timeout(DEADLINE, other)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refusal.code, ErrorCode::CapabilityDenied);
}

#[tokio::test]
async fn last_unsubscribe_wakes_one_pending_reader_and_a_later_subscription_gets_a_new_stream() {
    let (_root, state) = crate::testing::file_state();
    page(&state, "reader", 10, "@test/reader", &["device:list"]);
    let stream = subscribe(&state, "reader", 10, "device.*", 1).await;
    assert_eq!(
        subscribe(&state, "reader", 10, "device.changed", 2).await,
        stream
    );
    let mut receive = Box::pin(rpc(
        &state,
        "reader",
        10,
        "event.poll",
        json!({"streamId": stream}),
    ));
    pending(receive.as_mut()).await;
    let duplicate = rpc(
        &state,
        "reader",
        10,
        "event.poll",
        json!({"streamId": stream}),
    )
    .await
    .unwrap_err();
    assert_eq!(duplicate.code, ErrorCode::RateLimited);
    let close = |id| {
        rpc(
            &state,
            "reader",
            10,
            "event.unsubscribe",
            json!({"streamId": stream, "subscriptionId": id}),
        )
    };
    close(1).await.unwrap();
    pending(receive.as_mut()).await;
    close(2).await.unwrap();
    assert!(
        tokio::time::timeout(DEADLINE, receive)
            .await
            .unwrap()
            .unwrap()
            .is_null()
    );
    let next = subscribe(&state, "reader", 10, "device.changed", 1).await;
    close(1).await.unwrap(); // Late duplicate cleanup of the first stream.
    assert_ne!(next, stream);
    let mut receive = Box::pin(rpc(
        &state,
        "reader",
        10,
        "event.poll",
        json!({"streamId": next}),
    ));
    pending(receive.as_mut()).await;
    super::super::forget_window(&state, "reader");
    let refusal = tokio::time::timeout(DEADLINE, receive)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refusal.code, ErrorCode::CapabilityDenied);
}

#[tokio::test]
async fn resync_replaces_the_queued_history_after_overflow() {
    let (_root, state) = crate::testing::file_state();
    page(&state, "reader", 10, "@test/reader", &["device:list"]);
    let stream = subscribe(&state, "reader", 10, "device.changed", 1).await;
    for at in 1..=10 {
        device_change(&state, at);
    }
    let next = |method| rpc(&state, "reader", 10, method, json!({"streamId": stream}));
    let first: PluginEventEnvelope =
        serde_json::from_value(next("event.poll").await.unwrap()).unwrap();
    assert_eq!(first.seq, 3);
    let snapshot: PluginEventRecovery =
        serde_json::from_value(next("event.resync").await.unwrap()).unwrap();
    assert_eq!(snapshot.seq, 10);
    assert!(matches!(
        snapshot.events.as_slice(),
        [PluginEvent::DevicesChanged(_)]
    ));
    device_change(&state, 12);
    let after: PluginEventEnvelope =
        serde_json::from_value(next("event.poll").await.unwrap()).unwrap();
    assert_eq!(after.seq, 11);
}

mod transfers;
