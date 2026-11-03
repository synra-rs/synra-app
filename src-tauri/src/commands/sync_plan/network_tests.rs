use super::*;
use crate::state::AppState;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use synra_store::{Paired, sqlite::PluginVersionRecord};

const PLUGIN: &str = "@synra-plugin/touchpad";

fn install(state: &AppState) {
    state
        .db()
        .insert_plugin(PLUGIN, true, true, "active", 1_000, 1_000)
        .expect("installed");
    state
        .db()
        .record_plugin_version(&PluginVersionRecord {
            plugin_id: PLUGIN,
            version: "0.1.0",
            artifact: "plugin.wasm",
            digest: "same-digest",
            manifest: "{}",
            publisher_id: None,
            fingerprint: None,
            installed_ms: 1_000,
        })
        .expect("active version");
}

#[test]
fn a_sync_plan_uses_a_rediscovered_listener_after_the_peer_changes_ports() {
    let state = crate::testing::state();
    let remote = crate::testing::state();
    install(&state);
    install(&remote);
    let old = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("old listener");
    let old_address = old.local_addr().expect("old endpoint");
    let address = remote
        .host
        .lock()
        .listen_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .expect("new listener");
    drop(old);
    remote
        .host
        .lock()
        .listen_nonblocking()
        .expect("bounded accept");
    let peer = remote.host.lock().device_id();
    state
        .db()
        .pair_device(&Paired {
            device_id: peer.to_string(),
            display_name: "Android".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            address: Some(old_address.to_string()),
        })
        .expect("paired at old endpoint");
    state.host.lock().heard(
        &remote.announcement(address.port()),
        address,
        synra_store::sqlite::now_ms(),
    );
    let serving = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            if let Some(stream) = remote.host.lock().try_accept().expect("accept poll") {
                break stream;
            }
            assert!(Instant::now() < deadline, "no incoming inventory request");
            std::thread::sleep(Duration::from_millis(5));
        };
        let (parts, flags) = remote.host.lock().dialling_parts();
        let mut link = parts
            .accept_stream(stream, flags, unix_millis())
            .expect("Hello")
            .into_link();
        link.set_read_timeout(Some(Duration::from_secs(3)));
        let request = link.receive().expect("inventory request");
        let response = synra_core::plugin_sync::answer_plugin_list(
            &remote.db(),
            &request,
            peer,
            synra_store::sqlite::now_ms(),
        )
        .expect("inventory answer");
        link.send(&response).expect("answer sent");
    });
    let plan = plugins_sync_plan(&state, PLUGIN);
    serving.join().expect("bounded peer finished");
    let plan = plan.expect("sync plan");
    assert_eq!(plan.targets.len(), 1);
    assert!(plan.targets[0].online);
    assert!(
        !plan.targets[0].needs_artifact,
        "remote inventory came from the new endpoint"
    );
}
