//! First pairing through app commands with real TLS and independent file databases.

use super::*;
use std::time::{Duration, Instant};
use synra_core::peer::authenticated::AuthenticatedPeer;
use synra_protocol::pairing::PairingPhase;
use synra_testkit::authentication::NetworkFixture;

fn state(fixture: &NetworkFixture) -> AppState {
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
        crate::commands::transfers::local_peer(&identity),
    )
    .unwrap();
    state.host.lock().configure_network(Ok(network));
    state
}

fn terminal(state: &AppState) -> PairingSnapshot {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = pairing_status(state).unwrap().unwrap();
        if snapshot.phase.is_terminal() {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "pairing did not terminate: {snapshot:?}"
        );
        std::thread::yield_now();
    }
}

#[test]
fn app_commands_pair_unapproved_devices_and_persist_both_grants() {
    let a = NetworkFixture::unpaired(DeviceId::new(), "A").unwrap();
    let b = NetworkFixture::unpaired(DeviceId::new(), "B").unwrap();
    let a_state = state(&a);
    let b_state = state(&b);
    let a_id = a_state.identity.lock().device_id.to_string();
    let b_id = b_state.identity.lock().device_id.to_string();
    assert!(pairing_status(&a_state).unwrap().is_none());
    assert!(a.paired_devices().unwrap().is_empty());
    assert!(b.paired_devices().unwrap().is_empty());
    let invitation = pairing_invite(&a_state).unwrap();
    let listening = pairing_listen(&b_state, &a_id, &invitation).unwrap();
    assert_eq!(listening.phase, PairingPhase::Listening);
    a_state
        .db()
        .record_seen_device(&synra_store::Seen {
            device_id: b_id.clone(),
            display_name: "B".to_owned(),
            platform: "desktop".to_owned(),
            source: "127.0.0.1:1".to_owned(),
            seen_at_ms: a.now_ms as i64,
        })
        .unwrap();
    // The discovered port deliberately differs; only the imported reply selects the restricted port.
    pairing_connect(&a_state, &b_id, listening.reply.as_ref().unwrap()).unwrap();
    assert_eq!(terminal(&a_state).phase, PairingPhase::Paired);
    assert_eq!(terminal(&b_state).phase, PairingPhase::Paired);
    for (fixture, peer) in [(&a, b_id), (&b, a_id)] {
        let reopened = fixture.open_connection().unwrap();
        assert_eq!(reopened.paired_devices().unwrap().len(), 1);

        let reopened = parking_lot::Mutex::new(reopened);
        let approvals = synra_core::peer_trust::approved_peers(
            &reopened,
            &fixture.credentials,
            fixture.now_ms / 1000,
        )
        .unwrap();
        assert_eq!(approvals.len(), 1);
        let rows = reopened.lock().paired_devices().unwrap();
        assert!(rows.iter().any(|row| row.device_id == peer));
    }
}

#[test]
fn imports_must_match_the_selected_device_and_cancellation_names_the_live_generation() {
    let a = NetworkFixture::unpaired(DeviceId::new(), "A").unwrap();
    let b = NetworkFixture::unpaired(DeviceId::new(), "B").unwrap();
    let a_state = state(&a);
    let b_state = state(&b);
    let invitation = pairing_invite(&a_state).unwrap();
    let wrong = pairing_listen(&b_state, &DeviceId::new().to_string(), &invitation).unwrap_err();
    assert_eq!(wrong.code, ErrorCode::NotPaired);
    assert!(pairing_status(&b_state).unwrap().is_none());
    let snapshot = pairing_listen(
        &b_state,
        &a_state.identity.lock().device_id.to_string(),
        &invitation,
    )
    .unwrap();
    let absent = pairing_cancel(&b_state, &RequestId::new().to_string()).unwrap_err();
    assert_eq!(absent.code, ErrorCode::NotPaired);
    pairing_cancel(&b_state, &snapshot.request_id.to_string()).unwrap();
    assert_eq!(terminal(&b_state).phase, PairingPhase::Cancelled);
    assert!(b.paired_devices().unwrap().is_empty());
}

#[test]
fn malformed_identifiers_and_missing_native_identity_are_refused() {
    let state = crate::testing::state();
    assert_eq!(
        pairing_listen(&state, "invalid", "invitation")
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        pairing_cancel(&state, "invalid").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        pairing_invite(&state).unwrap_err().code,
        ErrorCode::NotPaired
    );
}
