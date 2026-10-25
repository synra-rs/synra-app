//! Tests for the shell's own discovery.
//!
//! # What is worth testing here
//!
//! Not the sockets: `synra-core` tests those against real UDP, and a second test of a broadcast would be a slower copy of
//! one that already exists. What is this module's own is the **join** — what it heard has to reach the table the devices
//! screen reads, and a scan has to publish a deadline the worker can see and take it back down before it answers. Those are
//! the two things that are wrong in a way a user notices.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use synra_core::discovery::Candidate;
use synra_protocol::discovery::TrustLevel;
use synra_protocol::ids::DeviceId;
use synra_protocol::session::PlatformKind;

use super::{Discovery, note_sighting, record};
use crate::commands::devices::devices_snapshot;

/// A device id that is shaped like one, so nothing here accidentally tests id parsing.
const PEER: &str = "33333333-3333-4333-8333-333333333333";

/// One sighting, as the worker hands it over.
fn candidate() -> Candidate {
    Candidate {
        device_id: PEER.parse::<DeviceId>().expect("a device id"),
        display_name: "Study PC".to_owned(),
        platform: Some(PlatformKind::Desktop),
        app_version: Some("0.1.0".to_owned()),
        claimed_features: Vec::new(),
        address: SocketAddr::from(([192, 168, 1, 7], 32_100)),
        trust: TrustLevel::Announced,
        last_seen_ms: 5_000,
    }
}

/// A worker on ports the operating system chose.
///
/// ★ Port zero, not the well-known discovery port: binding that would collide with a kernel that happens to be running on
/// this machine, and a test that needed it free would fail for a reason unrelated to what it asserts. It is the same
/// reason `DiscoveryService::bind_on` takes the port as an argument.
fn discovery(state: &crate::AppState) -> Discovery {
    Discovery::start(
        state.announcement(1),
        0,
        Arc::clone(&state.db),
        state.host.clone(),
        0,
        Arc::new(|_: &str| {}),
    )
    .expect("the sockets are free")
}

#[test]
fn a_scan_counts_devices_rather_than_packets() {
    // ★ The set is what makes `candidates_found` mean "devices that answered". A device announces more than once inside the
    // scan window, and a plain counter would report two for one machine — while the daemon, which reads a set, reports one.
    let sightings: super::Sightings =
        Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new()));

    note_sighting(&sightings, "aaaa");
    note_sighting(&sightings, "aaaa");
    note_sighting(&sightings, "bbbb");

    assert_eq!(
        sightings.lock().len(),
        2,
        "the repeat is the same device, not a second candidate"
    );
}

#[test]
fn a_deliberate_exit_asks_the_worker_to_stop() {
    // ★ The token is the whole mechanism: the worker is a loop on another thread, and the only thing a shutdown can share
    // with it is a value it reads each turn. Asserted here because `stop` is what the `ExitRequested` arm calls — a
    // method that cancelled a token nothing else held would look identical and do nothing.
    let state = crate::testing::state();
    let discovery = discovery(&state);

    assert!(
        !discovery.is_stopped(),
        "a worker that started has not been asked to stop"
    );
    discovery.stop();
    assert!(discovery.is_stopped(), "the worker was not asked to stop");
}

#[test]
fn what_discovery_hears_reaches_the_list_the_devices_screen_reads() {
    // ★ The whole reason the shell hosts the sockets itself.
    //
    // The sighting is written into **this** process's `devices_seen`, which is the table `devices_snapshot` reads — so the
    // page shows the device. A scan delegated to the daemon could not do this: the daemon keeps its own data directory and
    // its own table, so it would find three devices and this window would show none.
    let state = crate::testing::state();

    record(&state.db, &[candidate()]);

    let cards = devices_snapshot(&state).expect("a snapshot").devices;
    assert_eq!(cards.len(), 1, "the device that was heard is in the list");
    let card = &cards[0];
    assert_eq!(card.display_name, "Study PC");
    assert!(!card.paired, "a sighting is not a pairing");
    assert_eq!(
        card.address, None,
        "and a sighting offers no address to connect to"
    );
    assert_eq!(card.platform, "Desktop");
    assert_eq!(card.last_seen_ms, 5_000);
}

#[test]
fn a_scan_publishes_a_deadline_the_worker_reads_and_releases_it_before_answering() {
    let state = crate::testing::state();
    let discovery = discovery(&state);
    let deadline = discovery.on_demand();

    discovery.scan_with_wait(Duration::from_millis(60), 1_000, |window| {
        assert_eq!(
            deadline.load(Ordering::Relaxed),
            1_060,
            "the worker sees the deadline during the wait"
        );
        assert_eq!(
            window,
            Duration::from_millis(60),
            "the caller waits the requested window"
        );
    });
    assert_eq!(
        deadline.load(Ordering::Relaxed),
        0,
        "the deadline is released before the answer, so a caller that has gone stops the broadcast"
    );
}

#[test]
fn recording_a_sighting_names_what_it_wrote() {
    // ★ The return value is the announce hook's input, so what matters is that it names exactly the devices the table
    // accepted — an upsert of a device already seen still names it, because "heard it again" is a fact worth announcing
    // (the row just changed) and a device whose write failed must not be named at all.
    let state = crate::testing::state();

    assert_eq!(record(&state.db, &[candidate()]), vec![PEER.to_owned()]);
    assert_eq!(
        record(&state.db, &[candidate()]),
        vec![PEER.to_owned()],
        "a second sighting is still an announcement"
    );
}
