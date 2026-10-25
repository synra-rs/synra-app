//! Production app commands against the shared receiver over pinned TLS and file databases.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use synra_core::peer::authenticated::AuthenticatedPeer;
use synra_protocol::ids::DeviceId;
use synra_testkit::authentication::NetworkFixture;

use crate::AppState;
use crate::commands::transfers::{local_peer, transfer_send_file};

const LOCAL: &str = "11111111-1111-4111-8111-111111111111";
const PEER: &str = "22222222-2222-4222-8222-222222222222";

fn device(text: &str) -> DeviceId {
    text.parse().unwrap()
}

fn state(fixture: &NetworkFixture, address: SocketAddr) -> AppState {
    let state = AppState::new(
        fixture.open_connection().unwrap(),
        synra_store::Paths::under(fixture.root()),
        "test",
        fixture.now_ms as i64,
    )
    .unwrap();
    let identity = state.identity.lock().clone();
    let network = AuthenticatedPeer::new(
        Arc::clone(&state.db),
        Arc::clone(&fixture.credentials),
        local_peer(&identity),
    )
    .unwrap();
    state.host.lock().configure_network(Ok(network));
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: PEER.to_owned(),
            display_name: "Peer".to_owned(),
            platform: "desktop".to_owned(),
            features: "[]".to_owned(),
            link_token: "metadata-only".to_owned(),
            address: Some(address.to_string()),
            paired_at_ms: fixture.now_ms as i64,
        })
        .unwrap();
    state
}

fn host(fixture: &NetworkFixture) -> Arc<Mutex<synra_core::Host>> {
    let identity =
        synra_core::identity::Identity::load(fixture, "test", fixture.now_ms as i64).unwrap();
    let profile = local_peer(&identity);
    let network = AuthenticatedPeer::new(
        Arc::clone(&fixture.shared_db),
        Arc::clone(&fixture.credentials),
        profile.clone(),
    )
    .unwrap();
    let mut host = synra_core::Host::new(identity, profile);
    host.configure_network(Ok(network));
    Arc::new(Mutex::new(host))
}

fn router() -> Arc<synra_core::dispatch::orchestrate::Router> {
    let plugins = synra_core::dispatch::PluginSet::new(
        synra_plugin_host::Backend::new(Arc::new(synra_plugin_host::ScriptedBackend::new())),
        Arc::new(synra_plugin_host::testing::RefusingHost),
    );
    Arc::new(synra_core::dispatch::orchestrate::Router::new(
        Arc::new(Mutex::new(plugins)),
        Arc::new(Mutex::new(synra_core::intent::Board::new())),
    ))
}

fn accept(listener: &TcpListener) -> std::net::TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((socket, _)) => return socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("accept failed: {error}"),
        }
        assert!(Instant::now() < deadline, "no authenticated dial arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_app_file_command_reaches_the_production_receiver_and_waits_for_verification() {
    let local = NetworkFixture::new(device(LOCAL), device(PEER), "Local").unwrap();
    let remote = NetworkFixture::new(device(PEER), device(LOCAL), "Peer").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let state = state(&local, listener.local_addr().unwrap());
    let peer = host(&remote);
    let source = local.root().join("payload.bin");
    let content: Vec<_> = (0..=255u8).cycle().take(1_000_000).collect();
    std::fs::write(&source, &content).unwrap();
    let request = synra_ipc::dto::SendFileRequest {
        to: device(PEER),
        path: source.to_string_lossy().into_owned(),
        context_id: None,
    };
    let remote_root = remote.root().to_path_buf();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let desk = Arc::new(synra_core::pairing::PairingDesk::new(
                device(PEER),
                Duration::from_secs(3),
            ));
            synra_core::listener::handle_within(
                &peer,
                accept(&listener),
                &remote_root,
                Duration::from_secs(5),
                &desk,
                &router(),
                &synra_core::listener::no_answer(),
            )
        });
        let id = transfer_send_file(&state, &request).expect("the production receiver confirms");
        let served = worker.join().unwrap();
        assert_eq!(served.finished.len(), 1);
        let received = served.finished.first().unwrap();
        assert_eq!(received.transfer_id, Some(id));
        assert_eq!(received.peer, device(LOCAL));
        assert_eq!(std::fs::read(&received.path).unwrap(), content);
    });
    assert!(state.transfers.is_empty());
    assert!(source.exists(), "a user's source file survives sending");
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("transfers_total{kind=attachment,outcome=delivered}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
}

#[test]
fn a_production_receiver_refusal_never_records_delivery() {
    let local = NetworkFixture::new(device(LOCAL), device(PEER), "Local").unwrap();
    let remote = NetworkFixture::new(device(PEER), device(LOCAL), "Peer").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let state = state(&local, listener.local_addr().unwrap());
    let peer = host(&remote);
    let source = local.root().join("declined.bin");
    std::fs::write(&source, vec![7; 600_000]).unwrap();
    let remote_root = remote.root().to_path_buf();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let desk = Arc::new(synra_core::pairing::PairingDesk::new(
                device(PEER),
                Duration::from_secs(3),
            ));
            let offer: Arc<synra_core::inbound::OfferCallback> =
                Arc::new(|_| synra_core::inbound::OfferDecision::Refuse);
            let progress: Arc<synra_core::listener::ProgressCallback> = Arc::new(|_| {});
            let cancel: Arc<synra_core::listener::CancelCallback> = Arc::new(|_| false);
            synra_core::listener::handle_within_with(
                &peer,
                accept(&listener),
                &remote_root,
                Duration::from_secs(3),
                &desk,
                &router(),
                &synra_core::listener::no_answer(),
                &progress,
                &offer,
                &cancel,
                &synra_core::listener::no_kernel_events(),
            )
        });
        let error = transfer_send_file(
            &state,
            &synra_ipc::dto::SendFileRequest {
                to: device(PEER),
                path: source.to_string_lossy().into_owned(),
                context_id: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, synra_protocol::ErrorCode::TransferFailed);
        let served = worker.join().unwrap();
        assert!(served.finished.is_empty());
    });
    assert!(state.transfers.is_empty());
    let metrics = state.metrics.snapshot();
    assert_eq!(
        metrics.get("transfers_total{kind=attachment,outcome=failed}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
    assert!(
        metrics
            .get("transfers_total{kind=attachment,outcome=delivered}")
            .is_none()
    );
    assert!(metrics.get("transfer_bytes{kind=attachment}").is_none());
    assert!(source.exists());
}

#[test]
fn cancellation_at_the_production_receiver_closes_the_app_row_without_delivery() {
    let local = NetworkFixture::new(device(LOCAL), device(PEER), "Local").unwrap();
    let remote = NetworkFixture::new(device(PEER), device(LOCAL), "Peer").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let state = state(&local, listener.local_addr().unwrap());
    let peer = host(&remote);
    let source = local.root().join("cancelled.bin");
    std::fs::write(&source, vec![5; 600_000]).unwrap();
    let sender = synra_core::TransferSender::open(&source, synra_core::DEFAULT_CHUNK_SIZE).unwrap();
    let id = sender.transfer_id();
    let row = crate::commands::transfers::snapshot_from(
        &sender.request(),
        (device(PEER), "Peer"),
        "queued",
        0,
    );
    let token = synra_core::supervisor::Cancellation::new();
    let remote_root = remote.root().to_path_buf();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let desk = Arc::new(synra_core::pairing::PairingDesk::new(
                device(PEER),
                Duration::from_secs(3),
            ));
            let cancellation = token.clone();
            let progress: Arc<synra_core::listener::ProgressCallback> = Arc::new(move |event| {
                if event.progress.blocks_done > 0 {
                    cancellation.cancel();
                }
            });
            let offer: Arc<synra_core::inbound::OfferCallback> =
                Arc::new(synra_core::inbound::auto_accept);
            let cancel: Arc<synra_core::listener::CancelCallback> = Arc::new(|_| false);
            synra_core::listener::handle_within_with(
                &peer,
                accept(&listener),
                &remote_root,
                Duration::from_secs(3),
                &desk,
                &router(),
                &synra_core::listener::no_answer(),
                &progress,
                &offer,
                &cancel,
                &synra_core::listener::no_kernel_events(),
            )
        });
        assert!(
            crate::commands::transfers::send_over_with(
                &state,
                device(PEER),
                listener.local_addr().unwrap(),
                sender,
                row,
                Some(&token)
            )
            .is_err()
        );
        let served = worker.join().unwrap();
        assert!(served.finished.is_empty());
    });
    assert!(state.transfers.is_empty());
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("transfers_total{kind=attachment,outcome=cancelled}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
    assert!(source.exists());
    assert!(state.transfers.forget(&id).is_none());
}
