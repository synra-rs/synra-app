use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use synra_core::dispatch::{PluginSet, Router};
use synra_core::listener::{self, Served};
use synra_core::transfer::DEFAULT_CHUNK_SIZE;
use synra_ipc::{PluginTransferState, PluginTransferStatus};
use synra_plugin_host::{Backend, ScriptedBackend, testing::RefusingHost};
use synra_protocol::{DeviceId, TransferId};

use super::super::open_page;
use super::{
    AppState, ErrorCode, KernelEvent, json, page, plugin_ui_rpc, project_events, rpc, upload,
};

const DEADLINE: Duration = Duration::from_secs(10);
const OWNER: &str = "@test/tcp-owner";
const GRANTS: &[&str] = &["file:transfer:send"];

fn pair(state: &AppState, id: DeviceId, address: Option<SocketAddr>) {
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: id.to_string(),
            display_name: "Transfer test peer".into(),
            platform: "test".into(),
            features: "[]".into(),
            address: address.map(|address| address.to_string()),
            paired_at_ms: crate::testing::FIXED_NOW_MS,
        })
        .unwrap();
}

async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(DEADLINE, async {
        let mut readiness = tokio::time::interval(Duration::from_millis(5));
        loop {
            readiness.tick().await;
            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("blocking receive socket");
                    return stream;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    })
    .await
    .expect("plugin worker reached the loopback listener")
}

fn receive(state: &AppState, stream: TcpStream) -> Served {
    let set = PluginSet::new(
        Backend::new(Arc::new(ScriptedBackend::new())),
        Arc::new(RefusingHost),
    )
    .into_shared()
    .unwrap();
    let router = Arc::new(Router::new(set, Arc::clone(&state.board)));
    listener::handle_within(
        &state.host,
        stream,
        state.paths.data_dir(),
        DEADLINE,
        &router,
        &listener::no_answer(),
    )
}

fn status(state: &Arc<AppState>, token: &str, transfer: TransferId) -> PluginTransferStatus {
    let value = plugin_ui_rpc(
        state,
        token,
        "transfer.status",
        &json!({ "transferId": transfer }),
    )
    .unwrap();
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn plugin_upload_crosses_tcp_and_retains_the_verified_outcome_across_pages() {
    let (_sender_root, sender) = crate::testing::file_state();
    let (_receiver_root, receiver) = crate::testing::file_state();
    let sender = Arc::new(sender);
    let receiver = Arc::new(receiver);
    let local = sender.host.lock().device_id();
    let remote = receiver.host.lock().device_id();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    pair(&sender, remote, Some(listener.local_addr().unwrap()));
    pair(&receiver, local, None);
    page(&sender, "original", 20, OWNER, GRANTS);
    let watch = sender.events.subscribe();

    let bytes: Vec<u8> = (0..4 * DEFAULT_CHUNK_SIZE + 17)
        .map(|offset| (offset % 251) as u8)
        .collect();
    let id = upload(&sender, "20", remote, &bytes);
    let selector = json!({ "subscriptionId": 1, "transferId": id });
    let stream = rpc(&sender, "original", 20, "event.subscribe", selector.clone())
        .await
        .unwrap();
    assert_eq!(
        status(&sender, "20", id).state,
        PluginTransferState::Pending
    );

    let socket = accept(&listener).await;
    let target = Arc::clone(&receiver);
    let served = tokio::task::spawn_blocking(move || receive(&target, socket));
    let served = tokio::time::timeout(DEADLINE, served)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(served.peer, Some(local), "{:?}", served.ended);
    let [finished] = served.finished.as_slice() else {
        panic!("one verified attachment: {served:?}");
    };
    assert_eq!(finished.transfer_id, Some(id));

    let received = crate::commands::transfers::transfer_received(&receiver);
    let [received] = received.as_slice() else {
        panic!("one file in the receiver inbox");
    };
    assert_eq!(received.bytes, bytes.len() as u64);
    assert_eq!(std::fs::read(&received.path).unwrap(), bytes);

    let delivery = sender.events.take(watch).expect("worker events");
    assert!(delivery.events().iter().any(
        |event| matches!(event, KernelEvent::TransferFinished { transfer_id } if *transfer_id == id)
    ));
    project_events(&sender, &delivery, 25);
    let completed = status(&sender, "20", id);
    assert_eq!(completed.state, PluginTransferState::Completed);
    assert_eq!(completed.received_bytes, bytes.len() as u64);
    assert_eq!(completed.blocks_done, 5);
    assert_eq!(completed.blocks_total, 5);
    assert!(completed.ended_at_ms.is_some());
    let frame = tokio::time::timeout(
        DEADLINE,
        rpc(
            &sender,
            "original",
            20,
            "event.poll",
            json!({ "streamId": stream }),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(frame["payload"], json!(completed));

    crate::commands::plugin_ui::forget_window(&sender, "original");
    open_page(&sender, "reopened", 21, OWNER, GRANTS);
    let closed = rpc(
        &sender,
        "original",
        20,
        "event.poll",
        json!({ "streamId": stream }),
    )
    .await
    .unwrap_err();
    assert_eq!(closed.code, ErrorCode::CapabilityDenied);
    let reopened = rpc(&sender, "reopened", 21, "event.subscribe", selector)
        .await
        .unwrap();
    assert_ne!(stream, reopened);
    assert_eq!(status(&sender, "21", id), completed);
    let recovered = rpc(
        &sender,
        "reopened",
        21,
        "event.resync",
        json!({ "streamId": reopened }),
    )
    .await
    .unwrap();
    assert_eq!(
        recovered["events"],
        json!([{ "name": "transfer.progress", "payload": completed }])
    );
    sender.events.unsubscribe(watch);
}
