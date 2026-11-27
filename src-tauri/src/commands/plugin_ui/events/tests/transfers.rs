use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use synra_ipc::PluginTransferStatus;
use synra_ipc::dto::{PluginUploadAppend, PluginUploadOpened, PluginUploadSent};
use synra_protocol::{DeviceId, TransferId};

use super::{
    AppState, DEADLINE, ErrorCode, KernelEvent, json, page, pending, project_events, rpc, subscribe,
};
use crate::commands::plugin_ui::plugin_ui_rpc;

mod loopback;

fn send_to_refused_port(state: &Arc<AppState>) -> TransferId {
    let to = DeviceId::new();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: to.to_string(),
            display_name: "Offline peer".into(),
            platform: "linux".into(),
            features: "[]".into(),
            address: Some("127.0.0.1:1".into()),
            paired_at_ms: crate::testing::FIXED_NOW_MS,
        })
        .unwrap();
    upload(state, "20", to, b"a")
}

fn upload(state: &Arc<AppState>, token: &str, to: DeviceId, bytes: &[u8]) -> TransferId {
    let opened = plugin_ui_rpc(
        state,
        token,
        "transfer.begin",
        &json!({ "to": to, "fileName": "sample.bin", "totalBytes": bytes.len() }),
    )
    .unwrap();
    let opened: PluginUploadOpened = serde_json::from_value(opened).unwrap();
    let mut offset = 0_u64;
    for chunk in bytes.chunks(opened.chunk_bytes) {
        let ack = plugin_ui_rpc(
            state,
            token,
            "transfer.chunk",
            &json!({ "uploadId": opened.upload_id, "offset": offset, "data": STANDARD.encode(chunk) }),
        )
        .unwrap();
        let ack: PluginUploadAppend = serde_json::from_value(ack).unwrap();
        offset += chunk.len() as u64;
        assert_eq!(ack.written, offset);
    }
    let accepted = plugin_ui_rpc(
        state,
        token,
        "transfer.finish",
        &json!({ "uploadId": opened.upload_id }),
    )
    .unwrap();
    let accepted: PluginUploadSent = serde_json::from_value(accepted).unwrap();
    accepted.transfer_id
}

#[tokio::test]
async fn real_worker_failure_only_reaches_the_authorized_owner_and_survives_recovery() {
    let (_root, state) = crate::testing::file_state();
    let state = Arc::new(state);
    page(&state, "owner", 20, "@test/owner", &["file:transfer:send"]);
    page(&state, "other", 21, "@test/other", &["file:transfer:send"]);
    let owner = subscribe(&state, "owner", 20, "*.*", 1).await;
    let other = subscribe(&state, "other", 21, "transfer.*", 1).await;
    let watch = state.events.subscribe();
    let id = send_to_refused_port(&state);
    let refused = rpc(
        &state,
        "other",
        21,
        "event.subscribe",
        json!({ "transferId": id, "subscriptionId": 2 }),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params["reason"], "transfer_not_found");
    let mine = rpc(
        &state,
        "owner",
        20,
        "event.subscribe",
        json!({ "transferId": id, "subscriptionId": 2 }),
    )
    .await
    .unwrap();
    assert_eq!(mine, owner);

    let mut foreign = Box::pin(rpc(
        &state,
        "other",
        21,
        "event.poll",
        json!({ "streamId": other }),
    ));
    pending(foreign.as_mut()).await;
    let expected: PluginTransferStatus =
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let status = plugin_ui_rpc(
                    &state,
                    "20",
                    "transfer.status",
                    &json!({ "transferId": id }),
                )
                .unwrap();
                let status: PluginTransferStatus = serde_json::from_value(status).unwrap();
                if status.ended_at_ms.is_some() {
                    break status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("worker retained a terminal outcome");
    let delivery = state.events.take(watch).unwrap();
    project_events(&state, &delivery, 25);
    let actual = tokio::time::timeout(
        DEADLINE,
        rpc(
            &state,
            "owner",
            20,
            "event.poll",
            json!({ "streamId": owner }),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(actual["name"], "transfer.progress");
    assert_eq!(actual["payload"], json!(expected));
    assert_eq!(actual["payload"]["state"], "failed");
    assert!(actual["payload"].get("peerDevice").is_none());
    pending(foreign.as_mut()).await;

    let recovery = rpc(
        &state,
        "owner",
        20,
        "event.resync",
        json!({ "streamId": owner, "afterSeq": actual["seq"] }),
    )
    .await
    .unwrap();
    assert_eq!(
        recovery["events"],
        json!([{ "name": "transfer.progress", "payload": expected }])
    );
    let recovery = rpc(
        &state,
        "other",
        21,
        "event.resync",
        json!({ "streamId": other, "afterSeq": 0 }),
    )
    .await
    .unwrap();
    assert_eq!(recovery["events"], json!([]));
    state.events.unsubscribe(watch);
}

#[tokio::test]
async fn a_management_bus_gap_invalidates_the_page_even_without_a_retained_update() {
    let (_root, state) = crate::testing::file_state();
    page(
        &state,
        "reader",
        20,
        "@test/reader",
        &["file:transfer:send"],
    );
    let stream = subscribe(&state, "reader", 20, "transfer.*", 1).await;
    let watch = state.events.subscribe();
    for _ in 0..=synra_core::bus::SUBSCRIBER_QUEUE_CAPACITY {
        state.publish(KernelEvent::TransferFinished {
            transfer_id: TransferId::new(),
        });
    }
    let delivery = state.events.take(watch).unwrap();
    assert!(delivery.missed() > 0);
    project_events(&state, &delivery, 26);
    let poll = tokio::time::timeout(
        DEADLINE,
        rpc(
            &state,
            "reader",
            20,
            "event.poll",
            json!({ "streamId": stream }),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(poll, json!({ "resync": true }));
    let recovered = rpc(
        &state,
        "reader",
        20,
        "event.resync",
        json!({ "streamId": stream, "afterSeq": 0 }),
    )
    .await
    .unwrap();
    assert_eq!(recovered, json!({ "seq":0, "events": [] }));
    let mut poll = Box::pin(rpc(
        &state,
        "reader",
        20,
        "event.poll",
        json!({ "streamId": stream }),
    ));
    pending(poll.as_mut()).await;
    state.events.unsubscribe(watch);
}
