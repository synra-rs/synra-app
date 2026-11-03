use super::*;
use synra_protocol::discovery::DeviceAnnouncement;
use synra_protocol::handshake::PROTOCOL_VERSION;
use synra_protocol::session::PlatformKind;
use synra_store::Paired;

const PEER: &str = "33333333-3333-4333-8333-333333333333";

fn paired(state: &AppState, stored: Option<&str>) -> Paired {
    let row = Paired {
        device_id: PEER.to_owned(),
        display_name: "Android".to_owned(),
        platform: "android".to_owned(),
        features: "[]".to_owned(),
        paired_at_ms: 1_000,
        address: stored.map(str::to_owned),
    };
    state.db().pair_device(&row).expect("paired");
    row
}

fn announce(state: &AppState, port: u16, now: i64) {
    state.host.lock().heard(
        &DeviceAnnouncement {
            device_id: PEER.parse().expect("id"),
            display_name: "Android".to_owned(),
            protocol: PROTOCOL_VERSION,
            port,
            platform: Some(PlatformKind::Android),
            app_version: None,
            features: Vec::new(),
        },
        "127.0.0.1:60000".parse().expect("UDP source"),
        now,
    );
}

#[test]
fn rediscovery_replaces_a_saved_port_and_marks_an_idle_paired_device_online() {
    let state = crate::testing::state();
    paired(&state, Some("127.0.0.1:1"));
    announce(&state, 47_101, synra_store::sqlite::now_ms());
    let id = PEER.parse().expect("id");
    assert_eq!(
        address_of(&state, id).expect("current endpoint").port(),
        47_101
    );
    let snapshot = crate::commands::devices::devices_snapshot(&state).expect("snapshot");
    let card = snapshot
        .devices
        .iter()
        .find(|card| card.device_id == id)
        .expect("paired card");
    assert!(card.online);
    assert_eq!(card.address.as_deref(), Some("127.0.0.1:47101"));
    assert!(
        state.host.lock().sessions().is_empty(),
        "availability needs no artificial session"
    );
}

#[test]
fn an_inbound_pairing_with_no_saved_endpoint_can_use_discovery() {
    let state = crate::testing::state();
    paired(&state, None);
    announce(&state, 47_102, synra_store::sqlite::now_ms());
    assert_eq!(
        address_of(&state, PEER.parse().expect("id"))
            .expect("discovered")
            .port(),
        47_102
    );
}

#[test]
fn old_sightings_expire_and_a_goodbye_immediately_marks_the_peer_offline() {
    let state = crate::testing::state();
    paired(&state, Some("127.0.0.1:1"));
    let now = synra_store::sqlite::now_ms();
    announce(&state, 47_103, now);
    let row = state.db().paired_devices().expect("rows").remove(0);
    assert!(presence(&state.host, &row, now).online);
    assert!(!presence(&state.host, &row, now + 90_000).online);
    state
        .host
        .lock()
        .went_offline(PEER.parse().expect("id"), now);
    assert!(!presence(&state.host, &row, now).online);
}

#[test]
fn an_unpaired_or_blocked_announcement_cannot_authorize_a_dial() {
    let state = crate::testing::state();
    announce(&state, 47_104, synra_store::sqlite::now_ms());
    let id = PEER.parse().expect("id");
    assert!(address_of(&state, id).is_err());
    let row = paired(&state, Some("127.0.0.1:1"));
    state.db().block_device(&row).expect("blocked");
    assert!(address_of(&state, id).is_err());
    assert!(
        !crate::commands::devices::devices_snapshot(&state)
            .expect("snapshot")
            .devices[0]
            .online
    );
}
