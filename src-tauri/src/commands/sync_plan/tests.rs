//! What a plan says about each kind of peer.

use super::{PeerView, interpret, plan_for};
use synra_core::plugin_sync::{ConflictPolicy, RemoteState};
use synra_protocol::ids::{DeviceId, PluginId};
use synra_protocol::session::{PluginList, PluginSummary};

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

// ── What was recorded from an answer ──────────────────────────────────────────────────────────────

fn summary(id: &str, version: Option<&str>, digest: Option<&str>) -> PluginSummary {
    PluginSummary {
        plugin_id: PluginId::parse(id).expect("an id"),
        version: version.map(str::to_owned),
        digest: digest.map(str::to_owned),
        active: true,
    }
}

fn answer(plugins: Vec<PluginSummary>) -> PluginList {
    PluginList { plugins }
}

#[test]
fn the_plugins_own_state_comes_from_its_summary_and_every_held_row_is_recorded() {
    let id = PluginId::parse("@acme/hello").expect("an id");
    let list = answer(vec![
        summary("@acme/hello", Some("1.2.3"), Some("sha256-abc")),
        summary("@acme/other", Some("2.0.0"), Some("sha256-other")),
    ]);

    let (remote, held) = interpret(&id, &list);

    assert_eq!(remote.plugin_id.as_str(), "@acme/hello");
    assert_eq!(remote.version.as_deref(), Some("1.2.3"));
    assert_eq!(remote.digest.as_deref(), Some("sha256-abc"));
    assert_eq!(
        held,
        vec![
            ("@acme/hello".to_owned(), "1.2.3".to_owned(), "sha256-abc".to_owned()),
            ("@acme/other".to_owned(), "2.0.0".to_owned(), "sha256-other".to_owned()),
        ],
        "a plan about one plugin does not discard what the peer said about the others"
    );
}

#[test]
fn a_peer_that_answered_without_the_plugin_is_known_to_not_have_it() {
    // ★ The difference the plan depends on: an answer that does not mention the plugin is a fact ("it does not have it"), while
    // no answer at all is not. Both make `decide` push, but only one means the device could be reached.
    let id = PluginId::parse("@acme/hello").expect("an id");
    let list = answer(vec![summary("@acme/other", Some("2.0.0"), Some("sha256-other"))]);

    let (remote, held) = interpret(&id, &list);

    assert_eq!(remote, RemoteState::absent(id));
    assert_eq!(held.len(), 1, "the plugin it does have is still recorded");
}

#[test]
fn a_summary_that_claims_neither_a_version_nor_a_digest_is_not_recorded() {
    // A row with empty strings would read as "the peer holds the empty artifact", which `has_it` counts as holding.
    let id = PluginId::parse("@acme/hello").expect("an id");
    let list = answer(vec![
        summary("@acme/hello", None, None),
        summary("@acme/other", Some("2.0.0"), None),
    ]);

    let (remote, held) = interpret(&id, &list);

    assert_eq!(remote, RemoteState::absent(id), "a claim of nothing is not a claim to hold it");
    assert_eq!(
        held,
        vec![("@acme/other".to_owned(), "2.0.0".to_owned(), String::new())],
        "a version with no digest is still something it holds"
    );
}
