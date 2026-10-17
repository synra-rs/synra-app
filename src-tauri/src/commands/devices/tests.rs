//! Tests for the devices commands.
//!
//! # Paired, and heard from but not paired
//!
//! Both appear in a snapshot and both are exercised here, because the kernel writes `devices_seen` (the daemon's
//! discovery thread) as well as `paired_devices` (the handshake). An earlier version of this file could not cover the
//! second set at all — the table existed, the reader existed, and nothing wrote it — which is what a reader without a
//! writer looks like from the outside.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

use synra_ipc::{ScanOptions, ScanState};
use synra_protocol::ErrorCode;
use synra_store::{Paired, Seen};

use super::{devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot, scan_budget};
use crate::state::AppState;
use crate::testing;

const OTHER: &str = "22222222-2222-4222-8222-222222222333";

/// A state with one paired device in it.
fn with_paired_device() -> AppState {
    let state = testing::state();
    let identity = state.identity_snapshot(crate::KERNEL_VERSION);
    state

    .db()
    .pair_device(&Paired {
            device_id: identity.device_id.to_string(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            link_token: "token-for-tests".to_owned(),
            address: Some("192.168.1.4:5000".to_owned()),
        })
        .expect("pairs a device");
    state
}

#[test]
fn forgetting_a_device_tells_the_interface() {
    // ★ The event side of forgetting. Without it the card stays on the list until the page is reopened — the same silent
    // staleness the plugin commands were fixed for.
    let state = testing::state();
    state
        .db()
        .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Phone".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            link_token: "token-for-tests".to_owned(),
            address: None,
        })
        .expect("pairs a device");
    let subscriber = state.events.subscribe();

    super::devices_forget(&state, OTHER).expect("forgets it");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::DeviceRemoved { device_id }) => {
            assert_eq!(device_id.to_string(), OTHER, "and names the device that went");
        }
        other => panic!("expected a removal, got {other:?}"),
    }
}

#[test]
fn a_finished_scan_announces_the_cards_it_left_behind() {
    // ★ The publish the scan does after its network work, which no test could reach: extracted, it is called here on a
    // fixture with one paired device. The claim is the one that matters — a scan ends and the window is told what it
    // produced, one card per device, built by `devices_snapshot` rather than assembled separately.
    let state = with_paired_device();
    let subscriber = state.events.subscribe();

    super::announce_cards(&state);

    let delivery = state.events.take(subscriber).expect("a card was announced");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::DeviceCardUpdated(card)] => {
            assert_eq!(card.device_id.to_string(), state.identity.lock().device_id.to_string());
        }
        other => panic!("expected one card, got {other:?}"),
    }
}

#[test]
fn a_device_with_nothing_paired_shows_an_empty_list_and_not_an_error() {
    let snapshot = devices_snapshot(&testing::state()).expect("a snapshot");
    assert!(snapshot.devices.is_empty(), "nothing is paired, so nothing is listed");
    assert_eq!(snapshot.identity.display_name, snapshot.identity.display_name, "the identity is still there");
}

#[test]
fn the_snapshot_carries_this_device_so_the_screen_needs_one_call() {
    let snapshot = devices_snapshot(&testing::state()).expect("a snapshot");
    assert_eq!(snapshot.identity.kernel_version, crate::KERNEL_VERSION);
    assert!(snapshot.identity.name_is_default, "a fresh device's name was generated");
    assert_eq!(snapshot.scanning, None, "and nothing is scanning yet");
}

#[test]
fn a_paired_device_becomes_a_card_the_interface_can_render() {
    let snapshot = devices_snapshot(&with_paired_device()).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1);
    let card = &snapshot.devices[0];
    assert_eq!(card.display_name, "Study PC");
    assert!(card.paired, "it is on the list because it was paired");
    assert_eq!(card.address.as_deref(), Some("192.168.1.4:5000"), "a paired device keeps its address");
}

#[test]
fn a_paired_device_with_no_session_is_offline() {
    // ★ This line said `true` for several rounds while the comment above it said "a paired device with no open session
    // is offline". The comment was right.
    //
    // Every paired device was reported online, so the dashboard showed a green dot for a laptop that had been shut for a
    // week — and the first the user heard about it was a send that failed, which is the moment they can least do
    // anything about it. `SessionRegistry::live_for` is what answers the question the card is asking.
    //
    // ★ What this does not cover. The other direction — a live session making the card online — needs a session in the
    // registry, and this side has no way to open one: `SessionKey` is opaque to callers outside `synra-core` and the
    // registry is filled by the transport, which a shell test does not run. So this pins the value that was wrong and
    // not the query that replaced it; the query is `live_for`, one line, and this comment is where the next person
    // finds out nothing would go red if it were spelled differently.
    let state = with_paired_device();

    let snapshot = devices_snapshot(&state).expect("a snapshot");

    assert_eq!(snapshot.devices.len(), 1, "the fixture pairs one device");
    assert!(
        !snapshot.devices[0].online,
        "nothing has opened a session with this peer: {:?}",
        snapshot.devices[0]
    );
}

#[test]
fn devices_are_listed_by_name_when_the_names_differ() {
    let state = with_paired_device();
    state

    .db()
    .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Aardvark phone".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            link_token: "token-for-tests".to_owned(),
            address: None,
            paired_at_ms: 1_000,
        })
        .expect("pairs a second device");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices[0].display_name, "Aardvark phone", "sorted by name");
    assert_eq!(snapshot.devices[1].display_name, "Study PC");
}

#[test]
fn devices_with_the_same_name_still_come_out_in_one_order() {
    // ★ The tiebreaker is the identifier, and this is the test for it.
    //
    // Two devices with the same name must still come out in the same order on every device that has seen both, or a
    // difference in the list reads as one of the two lists being wrong. Sorting only by name leaves that to the database,
    // and SQLite is not obliged to agree with itself across runs.
    let state = with_paired_device();
    state

    .db()
    .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            link_token: "token-for-tests".to_owned(),
            address: None,
            paired_at_ms: 1_000,
        })
        .expect("pairs a second device");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(snapshot.devices[0].display_name, snapshot.devices[1].display_name, "with the same name");
    assert_ne!(snapshot.devices[0].device_id, snapshot.devices[1].device_id, "so the order came from the identifiers");
}

/// A sighting of a device that has never been paired, as discovery would record it.
fn sighting(id: &str, name: &str) -> Seen {
    Seen {
        device_id: id.to_owned(),
        display_name: name.to_owned(),
        platform: "linux".to_owned(),
        seen_at_ms: 3_000,
        source: "192.168.1.9:5000".to_owned(),
    }
}

#[test]
fn a_device_that_was_only_heard_from_still_reaches_the_list() {
    // ★ The card a user pairs from.
    //
    // Before the kernel wrote `devices_seen` this card could not exist, so the devices screen could only ever show
    // devices that were already paired — and "what can I pair with?" had no answer anywhere in the product.
    let state = testing::state();
    state.db().record_seen_device(&sighting(OTHER, "Hallway tablet")).expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1);
    let card = &snapshot.devices[0];
    assert_eq!(card.display_name, "Hallway tablet");
    assert!(!card.paired, "it has been heard from and not paired with");
}

#[test]
fn a_device_that_is_both_paired_and_heard_from_is_listed_once() {
    // ★ The duplicate this guards against.
    //
    // The two tables are separate and neither cascades into the other, so a paired device that discovery also hears will
    // be in both. Without the de-duplication it would appear twice — once as paired, once as not — and the user would
    // see the same machine twice with different affordances.
    let state = with_paired_device();
    let paired_id = devices_snapshot(&state).expect("a snapshot").devices.remove(0).device_id.to_string();
    state.db().record_seen_device(&sighting(&paired_id, "Study PC")).expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1, "listed once");
    assert!(snapshot.devices[0].paired, "and as paired, which is the more capable answer");
}

#[test]
fn paired_devices_are_listed_before_the_ones_that_still_need_pairing() {
    // ★ The first sort key.
    //
    // The devices a user can already send to are what the screen is for; the ones they have to pair with next are what
    // they are shopping for. The names are chosen so that sorting by name alone would put them the other way round.
    let state = with_paired_device();
    state.db().record_seen_device(&sighting(OTHER, "Aardvark tablet")).expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(snapshot.devices[0].display_name, "Study PC", "the paired one, despite sorting later by name");
    assert!(snapshot.devices[0].paired);
    assert_eq!(snapshot.devices[1].display_name, "Aardvark tablet");
    assert!(!snapshot.devices[1].paired);
}

#[test]
fn an_unpaired_device_is_shown_without_an_address() {
    // ★ Why `address` is absent for these and present for paired ones.
    //
    // The store keeps an address for a paired device because that is where it is reachable. For a device seen once over
    // discovery, the address in that announcement is where it *was*, which is not a promise about now — and showing it
    // on a card that looks exactly as usable as a paired one offers the user a device they cannot reach.
    let state = testing::state();
    state.db().record_seen_device(&sighting(OTHER, "Hallway tablet")).expect("records a sighting");
    assert_eq!(devices_snapshot(&state).expect("a snapshot").devices[0].address, None);
}

#[test]
fn a_paired_device_keeps_its_address() {
    let snapshot = devices_snapshot(&with_paired_device()).expect("a snapshot");
    assert_eq!(
        snapshot.devices[0].address.as_deref(),
        Some("192.168.1.4:5000"),
        "a paired device is reachable at a known address"
    );
}

/// A state whose shell bound the discovery sockets, on ports the operating system chose.
///
/// ★ Port zero rather than the well-known discovery port: binding that would collide with a kernel that happens to be
/// running on this machine, and a test that needed it free would fail for a reason unrelated to what it asserts. The
/// deadline is handed back because a scan publishes one, waits, and takes it down again — so watching it is the only way
/// to see that the worker was asked at all.
fn with_discovery() -> (AppState, Arc<AtomicI64>) {
    let state = testing::state();
    let discovery = crate::discovery::Discovery::start(
        state.announcement(1),
        0,
        Arc::clone(&state.db),
        state.host.clone(),
        0,
    )
    .expect("the sockets are free");
    let deadline = discovery.on_demand();
    (state.with_discovery(discovery), deadline)
}

#[test]
fn an_active_scan_without_a_discovery_worker_is_refused_rather_than_reported_as_running() {
    // ★ The command that used to fill a progress bar for a scan nobody performed.
    //
    // It answered with an identifier and a state carrying `elapsed_ms: 0, found: 0` that **nothing ever updated**, so the
    // interface drew a bar that could never move. "Found nothing" is true; "is scanning" was not, and this module's rule
    // is that a command answers or says why it cannot.
    //
    // This fixture is a shell with no discovery worker: the sockets are bound at start-up, and a machine with no network,
    // a firewall and a sandbox all produce the same bind failure. That is the one case left in which a network scan is
    // refused — the refusal names the reason, and the interface falls back to the cached read.
    let state = testing::state();
    assert!(state.discovery.is_none(), "the fixture is a shell that could not bind the sockets");

    let refused = devices_scan_start(&state, &ScanOptions::default()).expect_err("this state cannot scan");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    let reason = refused.params.get("reason").and_then(serde_json::Value::as_str).unwrap_or_default();
    assert!(reason.contains("discovery sockets"), "the refusal names what is missing: {reason}");
    // And nothing was left in the snapshot claiming a scan is under way.
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning, None);
}

#[test]
fn a_scan_budget_is_the_protocols_and_not_the_callers() {
    // A scan is a broadcast to every machine on the network, so its length is the protocol's to set. The two interesting
    // answers are the ceiling — a caller does not get to ask for a longer broadcast — and the default, which is the
    // window "look now" means when nobody said how long.
    let asked = |ms: Option<u64>| scan_budget(&ScanOptions { timeout_ms: ms, ..ScanOptions::default() });

    assert_eq!(asked(None), synra_protocol::limits::SCAN_PASSIVE);
    assert_eq!(asked(Some(10)), Duration::from_millis(10), "a shorter scan is honoured");
    assert_eq!(asked(Some(60_000)), synra_protocol::limits::SCAN_TOTAL, "a longer one is clamped");
    assert!(asked(Some(0)).is_zero(), "and zero is the protocol's way of saying `answer from what you know`");
}

#[test]
fn a_network_scan_records_itself_while_it_runs_and_clears_that_before_it_answers() {
    // ★ The state's own meaning, asserted from outside the command: `scanning` is *the scan in progress*. That is what the
    // interface reads to disable the button and draw the bar, so a scan that records nothing looks like a dead button, and
    // one that never clears looks like a button that has died.
    let (state, deadline) = with_discovery();
    let running = AtomicBool::new(false);
    let published = AtomicBool::new(false);

    std::thread::scope(|scope| {
        // A watcher borrowing the state, because the command blocks: a scan this short is only observable while it waits.
        scope.spawn(|| {
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                if state.scanning.lock().as_ref().is_some_and(|scan| scan.total_ms == 60) {
                    running.store(true, Ordering::Relaxed);
                }
                if deadline.load(Ordering::Relaxed) != 0 {
                    published.store(true, Ordering::Relaxed);
                }
                if running.load(Ordering::Relaxed) && published.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        let scan_id = devices_scan_start(&state, &ScanOptions { timeout_ms: Some(60), ..ScanOptions::default() })
            .expect("a scan with a worker behind it");

        assert!(!scan_id.is_empty(), "the contract answers with an identifier");
    });

    assert!(running.load(Ordering::Relaxed), "a snapshot taken during the scan says one is in progress");
    assert!(published.load(Ordering::Relaxed), "and the worker was asked to look now");
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning, None, "and it is over when it answers");
    assert_eq!(deadline.load(Ordering::Relaxed), 0, "with the deadline released, so nothing keeps broadcasting");
}

#[test]
fn a_cached_only_scan_finishes_immediately_rather_than_showing_a_bar_that_cannot_move() {
    let state = testing::state();

    let scan_id = devices_scan_start(&state, &cached()).expect("a scan");
    assert!(!scan_id.is_empty(), "the contract answers with an identifier");

    // ★ `None` is what "nothing to wait for" looks like on the wire, and `Some` is what the interface reads as *a scan is
    // running* — it disables the Scan button and draws the bar. This used to assert `total_ms == 0` on a state that
    // existed, which is the reading the interface contradicts.
    assert_eq!(
        devices_snapshot(&state).expect("a snapshot").scanning,
        None,
        "a scan that finishes before it answers is not a scan in progress"
    );
}

#[test]
fn the_options_the_interface_sends_deserialize_into_this_commands_argument() {
    // ★ The argument half of the contract, asserted on the side that can assert it.
    //
    // `DevicesPage.vue` sends `{ opts: { includePaired: false, cachedOnly: true } }`, and Tauri deserializes the inner
    // object into `ScanOptions` *before* this module is reached. `ScanOptions` denies unknown fields and requires both
    // flags, so a payload that drifted — a renamed key, a missing flag — fails in the bridge and arrives at the interface
    // as a transport failure ("The interface could not reach the kernel.") rather than as a refusal. That is exactly the
    // shape of the bug this test was written after: the interface had sent **no** `opts` at all.
    let sent = serde_json::json!({
        "includePaired": false,
        "cachedOnly": true,
    });

    let parsed: ScanOptions = serde_json::from_value(sent).expect("the interface's payload is this struct");
    assert!(parsed.cached_only, "the interface asks for the scan this shell can answer");
    assert!(!parsed.include_paired);
    assert_eq!(parsed.timeout_ms, None);
}

/// The only scan this shell can start.
///
/// ★ It finishes before it answers and records nothing, which is why the tests about *cancelling* no longer set up with
/// it: there is no scan in progress to cancel. They install the state directly instead, in [`scanning`].
fn cached() -> ScanOptions {
    ScanOptions { cached_only: true, ..ScanOptions::default() }
}

/// A scan that is genuinely running, put in place directly.
///
/// ★ This is the setup the cancellation tests used to reach through the command, and no longer can. An active scan is
/// refused for the reason [`devices_scan_start`] gives, and a cached one is over before it answers — so **nothing in this
/// shell can leave a scan in progress**, which is a fact about the shell rather than about cancelling. What is written
/// here is the state a future active scan would leave, and `devices_scan_cancel` is tested against it rather than
/// against a scan that never ran.
fn scanning(state: &AppState, scan_id: &str) {
    *state.scanning.lock() = Some(ScanState {
        scan_id: scan_id.to_owned(),
        elapsed_ms: 0,
        total_ms: 5_000,
        found: 0,
    });
}

#[test]
fn a_cancelled_scan_is_gone_from_the_next_snapshot() {
    let state = testing::state();
    scanning(&state, "scan-1");
    devices_scan_cancel(&state, "scan-1").expect("cancels");
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning, None);
}

#[test]
fn cancelling_a_scan_that_is_not_running_says_so_and_names_what_is() {
    let state = testing::state();
    scanning(&state, "scan-1");

    let error = devices_scan_cancel(&state, "scan-999").expect_err("the wrong scan");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        error.params["running"].as_str(),
        Some("scan-1"),
        "the refusal names the scan that is actually running, so a stale interface can correct itself"
    );
}

#[test]
fn cancelling_when_nothing_is_running_is_refused_rather_than_silently_accepted() {
    let error = devices_scan_cancel(&testing::state(), "scan-1").expect_err("nothing is running");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn two_scans_in_a_row_get_different_identifiers() {
    // The identifier is a counter, and the only reason it is not a random value is that it has to be readable in a
    // log. A counter that repeats would make the log line ambiguous, which defeats that reason.
    let state = testing::state();
    let first = devices_scan_start(&state, &cached()).expect("a scan");
    let second = devices_scan_start(&state, &cached()).expect("a scan");
    assert_ne!(first, second);
}

#[test]
fn a_forgotten_device_is_gone_from_the_next_snapshot() {
    let state = with_paired_device();
    let card = devices_snapshot(&state).expect("a snapshot").devices.remove(0);
    let id = card.device_id.to_string();

    devices_forget(&state, &id).expect("forgets");
    assert!(devices_snapshot(&state).expect("a snapshot").devices.is_empty(), "the pairing is gone");
}

#[test]
fn forgetting_survives_a_restart() {
    let (root, state) = testing::file_state();
    state

    .db()
    .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            link_token: "token-for-tests".to_owned(),
            address: None,
        })
        .expect("pairs");

    devices_forget(&state, OTHER).expect("forgets");
    let reopened = testing::reopen(&state);
    assert!(
        devices_snapshot(&reopened).expect("a snapshot").devices.is_empty(),
        "and it did not come back when the shell started again"
    );
    drop(root);
}

#[test]
fn forgetting_a_device_that_does_not_parse_is_the_callers_fault() {
    let error = devices_forget(&testing::state(), "not-a-device").expect_err("a bad identifier");
    assert_eq!(
        error.code,
        ErrorCode::InvalidArgument,
        "the caller sent it and the caller can fix it — unlike a corrupt row, which is nobody's"
    );
}

#[test]
fn forgetting_an_unknown_device_succeeds_and_removes_nothing() {
    // Not an error: the user's intent — "this device is not here" — is satisfied. Refusing would make a retry loop
    // possible on a state that is already the one the user asked for.
    devices_forget(&testing::state(), OTHER).expect("forgetting nothing is still forgetting");
}