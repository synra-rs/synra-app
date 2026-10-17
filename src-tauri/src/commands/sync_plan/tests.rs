//! What a plan says about each kind of peer.

use super::{PeerView, plan_for};
use synra_core::plugin_sync::{ConflictPolicy, RemoteState};
use synra_protocol::ids::{DeviceId, PluginId};

/// A peer holding this version and this digest, or nothing when both are `None`.
fn peer(name: &str, online: bool, version: Option<&str>, digest: Option<&str>) -> PeerView {
    let remote = (version.is_some() || digest.is_some()).then(|| RemoteState {
        plugin_id: PluginId::parse("@acme/hello").expect("an id"),
        version: version.map(str::to_owned),
        digest: digest.map(str::to_owned),
        active: false,
    });
    PeerView { device_id: DeviceId::new(), display_name: name.to_owned(), online, remote }
}

fn plan(peers: &[PeerView]) -> synra_ipc::dto::SyncPlanDto {
    plan_for(
        PluginId::parse("@acme/hello").expect("an id"),
        "1.2.3",
        "sha256-mine",
        peers,
        ConflictPolicy::SourceWins,
    )
}

#[test]
fn a_peer_with_the_same_bytes_needs_nothing() {
    let wanted = plan(&[peer("Study PC", true, Some("1.2.3"), Some("sha256-mine"))]);

    assert_eq!(wanted.targets.len(), 1);
    assert!(!wanted.targets[0].needs_artifact, "the same bytes are the same plugin");
}

#[test]
fn a_peer_with_different_bytes_needs_the_artifact() {
    // Same version string, different digest: the case a version-only comparison would call "already synced".
    let wanted = plan(&[peer("Study PC", true, Some("1.2.3"), Some("sha256-theirs"))]);

    assert!(wanted.targets[0].needs_artifact, "different bytes are a different artifact");
}

#[test]
fn a_peer_that_did_not_answer_is_reported_as_offline_and_unsynced() {
    // ★ "We could not ask" is not "it has it": dropping the row would make the panel claim a sync that never happened.
    let wanted = plan(&[peer("Study PC", false, None, None)]);

    assert_eq!(wanted.targets.len(), 1, "the device is still listed");
    assert!(wanted.targets[0].needs_artifact);
    assert!(!wanted.targets[0].online, "and the interface can see why it has not happened");
    assert_eq!(wanted.targets[0].version, None);
}

#[test]
fn the_plan_names_the_plugin_and_every_device_asked() {
    let wanted = plan(&[
        peer("Study PC", true, Some("1.2.3"), Some("sha256-mine")),
        peer("Laptop", true, None, None),
    ]);

    assert_eq!(wanted.plugin_id.as_str(), "@acme/hello");
    assert_eq!(wanted.targets.len(), 2, "one row per device asked");
    assert_eq!(wanted.targets[0].display_name, "Study PC");
    assert_eq!(wanted.targets[1].display_name, "Laptop");
}
