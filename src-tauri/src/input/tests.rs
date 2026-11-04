use super::*;
use synra_store::Paired;

const PLUGIN: &str = "@synra-plugin/touchpad";

#[cfg(all(feature = "touchpad-plugin-tests", windows))]
mod official_touchpad;

fn fixture() -> (crate::AppState, DeviceId, PluginId) {
    let state = crate::testing::state();
    let peer = DeviceId::new();
    state
        .db()
        .pair_device(&Paired {
            device_id: peer.to_string(),
            display_name: "Phone".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1000,
            address: None,
        })
        .unwrap();
    state
        .db()
        .insert_plugin(PLUGIN, true, true, "active", 1000, 1000)
        .unwrap();
    (state, peer, PluginId::parse(PLUGIN).unwrap())
}

#[test]
fn pairing_and_an_enabled_plugin_do_not_imply_mouse_control_grants() {
    let (state, peer, plugin) = fixture();
    let policy = LocalPolicy {
        db: Arc::clone(&state.db),
        host: Arc::downgrade(&state.host),
    };
    assert!(policy.authorize(peer, &plugin, false).is_err());
    state
        .db()
        .record_grants(PLUGIN, &[("device:input".to_owned(), true)], 1001, "test")
        .unwrap();
    assert!(
        policy.authorize(peer, &plugin, false).is_err(),
        "a concrete device send scope is also required"
    );
    state
        .db()
        .record_grants(
            PLUGIN,
            &[(format!("device:send:{peer}"), true)],
            1002,
            "test",
        )
        .unwrap();
    let revision = policy.authorize(peer, &plugin, false).unwrap();
    state
        .db()
        .record_grants(PLUGIN, &[("device:input".to_owned(), false)], 1003, "test")
        .unwrap();
    assert!(policy.authorize(peer, &plugin, false).is_err());
    assert_ne!(
        state
            .db()
            .plugin_authority(PLUGIN)
            .unwrap()
            .unwrap()
            .revision,
        revision
    );
}

#[test]
fn an_unknown_or_blocked_peer_cannot_request_input() {
    let (state, peer, plugin) = fixture();
    let policy = LocalPolicy {
        db: Arc::clone(&state.db),
        host: Arc::downgrade(&state.host),
    };
    state
        .db()
        .record_grants(
            PLUGIN,
            &[
                ("device:input".to_owned(), true),
                ("device:send:*".to_owned(), true),
            ],
            1001,
            "test",
        )
        .unwrap();
    assert!(policy.authorize(DeviceId::new(), &plugin, false).is_err());
    let paired = Paired {
        device_id: peer.to_string(),
        display_name: "Phone".to_owned(),
        platform: "android".to_owned(),
        features: "[]".to_owned(),
        paired_at_ms: 1000,
        address: None,
    };
    state.db().block_device(&paired).unwrap();
    assert!(policy.authorize(peer, &plugin, false).is_err());
}

#[test]
fn attaching_the_receiver_does_not_keep_the_host_or_database_alive() {
    let state = crate::testing::state();
    let host = Arc::downgrade(&state.host);
    let db = Arc::downgrade(&state.db);
    drop(state);
    assert!(host.upgrade().is_none());
    assert!(db.upgrade().is_none());
}
