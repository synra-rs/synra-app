use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use synra_core::{Host, LocalPeer, shared};
use synra_ipc::{PairingOutcome, PairingPhase};
use synra_plugin_host::{Backend, ScriptedBackend, testing::RefusingHost};
use synra_protocol::session::PlatformKind;

use super::*;
use crate::{commands::devices::devices_snapshot, testing};

const LIMIT: Duration = Duration::from_secs(5);

struct Pair {
    _a_root: testing::Root,
    _b_root: testing::Root,
    a: Arc<AppState>,
    b: Arc<AppState>,
    a_changed: mpsc::Receiver<()>,
    b_changed: mpsc::Receiver<()>,
    served: mpsc::Receiver<synra_core::listener::Served>,
}

fn peer(name: &str, platform: PlatformKind) -> (testing::Root, Arc<AppState>, mpsc::Receiver<()>) {
    let (root, mut state) = testing::file_state();
    let mut identity = state.identity.lock().clone();
    identity.display_name = name.to_owned();
    let local = LocalPeer::new(identity.device_id, name, platform, "0.1.0");
    state.host = shared(Host::new(identity, local));
    let (changed, receiver) = mpsc::channel();
    state.host.lock().pairing().set_sink(Arc::new(move || {
        let _ = changed.send(());
    }));
    (root, Arc::new(state), receiver)
}

fn pair() -> Pair {
    pair_with(PlatformKind::Desktop, PlatformKind::Android)
}

fn pair_with(a_platform: PlatformKind, b_platform: PlatformKind) -> Pair {
    let a_name = if a_platform == PlatformKind::Android {
        "安卓"
    } else {
        "PC"
    };
    let b_name = if b_platform == PlatformKind::Android {
        "安卓"
    } else {
        "PC"
    };
    let (a_root, a, a_changed) = peer(a_name, a_platform);
    let (b_root, b, b_changed) = peer(b_name, b_platform);
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let address = listener.local_addr().expect("the address");
    let announcement = b.host.lock().local_peer().announcement(address.port());
    a.host
        .lock()
        .heard(&announcement, address, synra_store::sqlite::now_ms());
    a.db()
        .record_seen_device(&synra_store::Seen {
            device_id: announcement.device_id.to_string(),
            display_name: b_name.to_owned(),
            platform: platform_text(b_platform),
            seen_at_ms: synra_store::sqlite::now_ms(),
            source: "udp".to_owned(),
        })
        .expect("seen");
    let server = Arc::clone(&b);
    let (finished, served) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + LIMIT;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "the dial never reached the listener"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        stream
            .set_nonblocking(false)
            .expect("blocking handshake on Windows");
        let plugins = synra_core::dispatch::PluginSet::new(
            Backend::new(Arc::new(ScriptedBackend::new())),
            Arc::new(RefusingHost),
        )
        .into_shared()
        .expect("a plugin set");
        let router = Arc::new(synra_core::dispatch::Router::new(
            plugins,
            Arc::clone(&server.board),
        ));
        let result = synra_core::listener::handle_within(
            &server.host,
            stream,
            &server.paths.data_dir(),
            Duration::from_secs(1),
            &router,
            &synra_core::listener::no_answer(),
        );
        if let Some(error) = &result.ended {
            eprintln!("pairing test listener ended: {error}");
        }
        let _ = finished.send(result);
    });
    Pair {
        _a_root: a_root,
        _b_root: b_root,
        a,
        b,
        a_changed,
        b_changed,
        served,
    }
}

fn wait_until(changed: &mpsc::Receiver<()>, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !condition() {
        changed
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the pairing state changed before its deadline");
    }
}

impl Pair {
    fn begin(&self) -> PairingSnapshot {
        let id = self.b.host.lock().device_id().to_string();
        let offer = devices_pair(&self.a, &id).expect("a waiting offer");
        assert_eq!(offer.phase, PairingPhase::Waiting);
        wait_until(&self.b_changed, || {
            snapshot_of(&self.b.host.lock().pairing()).phase == PairingPhase::Asked
        });
        let incoming = devices_pairing_snapshot(&self.b).expect("the incoming offer");
        assert_eq!(
            offer.offer.as_ref().unwrap().code,
            incoming.ask.as_ref().unwrap().code
        );
        assert_eq!(
            incoming.ask.unwrap().display_name,
            self.a.host.lock().local_peer().display_name
        );
        assert_eq!(
            devices_pairing_snapshot(&self.a).unwrap().phase,
            PairingPhase::Waiting
        );
        assert!(self.a.db().paired_devices().unwrap().is_empty());
        assert!(
            self.b.db().paired_devices().unwrap().is_empty(),
            "Hello must not pair either device"
        );
        offer
    }

    fn finish(&self, outcome: PairingOutcome) {
        wait_until(&self.a_changed, || {
            snapshot_of(&self.a.host.lock().pairing()).outcome == Some(outcome)
        });
        self.served
            .recv_timeout(LIMIT)
            .expect("the connection worker finished");
    }
}

#[test]
fn pc_and_android_wait_for_the_receivers_consent_before_both_records_are_written() {
    let pair = pair();
    pair.begin();
    devices_pairing_respond(&pair.b, true, false).expect("accept");
    pair.finish(PairingOutcome::Paired);
    let b_id = pair.b.host.lock().device_id().to_string();
    wait_until(&pair.a_changed, || {
        pair.a.db().is_paired_with(&b_id).unwrap()
    });
    assert!(
        pair.b
            .db()
            .is_paired_with(&pair.a.host.lock().device_id().to_string())
            .unwrap()
    );
    assert_eq!(pair.a.db().paired_devices().unwrap()[0].platform, "android");
    assert!(devices_snapshot(&pair.a).unwrap().devices[0].paired);
    assert!(devices_snapshot(&pair.b).unwrap().devices[0].paired);
}

#[test]
fn refusal_reaches_the_requester_and_neither_device_becomes_paired() {
    let pair = pair();
    pair.begin();
    devices_pairing_respond(&pair.b, false, false).expect("refuse");
    pair.finish(PairingOutcome::Refused);
    assert!(pair.a.db().paired_devices().unwrap().is_empty());
    assert!(pair.b.db().paired_devices().unwrap().is_empty());
}

#[test]
fn android_can_initiate_and_the_pc_must_accept_the_same_code() {
    let pair = pair_with(PlatformKind::Android, PlatformKind::Desktop);
    pair.begin();
    devices_pairing_respond(&pair.b, true, false).expect("PC accepts");
    pair.finish(PairingOutcome::Paired);
    let b_id = pair.b.host.lock().device_id().to_string();
    wait_until(&pair.a_changed, || {
        pair.a.db().is_paired_with(&b_id).unwrap()
    });
    assert_eq!(pair.b.db().paired_devices().unwrap()[0].platform, "android");
    assert_eq!(pair.a.db().paired_devices().unwrap()[0].platform, "desktop");
}

#[test]
fn cancellation_withdraws_the_receivers_prompt_without_pairing() {
    let pair = pair();
    pair.begin();
    devices_pairing_cancel(&pair.a).expect("cancel");
    pair.finish(PairingOutcome::Cancelled);
    assert_eq!(
        devices_pairing_snapshot(&pair.b).unwrap().outcome,
        Some(PairingOutcome::Cancelled)
    );
    assert!(pair.a.db().paired_devices().unwrap().is_empty());
    assert!(pair.b.db().paired_devices().unwrap().is_empty());
}

#[test]
fn blocking_refuses_the_request_and_survives_a_restart_without_granting_pairing() {
    let pair = pair();
    pair.begin();
    devices_pairing_respond(&pair.b, false, true).expect("block");
    pair.finish(PairingOutcome::Refused);
    let restarted = testing::reopen(&pair.b);
    let a_id = pair.a.host.lock().device_id().to_string();
    assert!(!restarted.db().is_paired_with(&a_id).unwrap());
    let card = devices_snapshot(&restarted).unwrap().devices.remove(0);
    assert!(card.blocked);
    assert!(!card.paired);
    assert!(pair.a.db().paired_devices().unwrap().is_empty());
}

#[test]
fn an_already_paired_device_is_rejected_before_dialling() {
    let (_root, state) = testing::file_state();
    let id = DeviceId::new().to_string();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: id.clone(),
            display_name: "安卓".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            address: Some(SocketAddr::from(([127, 0, 0, 1], 9)).to_string()),
            paired_at_ms: 1,
        })
        .unwrap();
    let error = devices_pair(&Arc::new(state), &id).expect_err("already paired");
    assert!(error.params.to_string().contains("already paired"));
}
