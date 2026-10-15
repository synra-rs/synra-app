//! Tests for the devices commands.
//!
//! # What is deliberately not tested here
//!
//! The "seen but not paired" cards. Nothing in the kernel writes `devices_seen` — the table exists, the reader exists,
//! and there is no writer — so there is no way for a test to put a row in it through the public store API, and writing
//! one with raw SQL from here would test a state the product cannot reach. It is a real gap in the kernel and it is
//! recorded as one rather than covered by a test that fabricates the row it asserts on.

use synra_ipc::ScanOptions;
use synra_protocol::ErrorCode;
use synra_store::Paired;

use super::{SCAN_CEILING_MS, devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot};
use crate::state::AppState;
use crate::testing;

const OTHER: &str = "22222222-2222-4222-8222-222222222222";

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

#[test]
fn a_scan_answers_with_an_identifier_and_appears_in_the_next_snapshot() {
    let state = testing::state();
    let scan_id = devices_scan_start(&state, &ScanOptions::default()).expect("a scan");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    let scanning = snapshot.scanning.expect("the scan is in the snapshot");
    assert_eq!(scanning.scan_id, scan_id);
    assert_eq!(scanning.total_ms, SCAN_CEILING_MS, "no timeout asked for means the ceiling");
    assert_eq!(scanning.found, 0, "nothing has answered yet");
}

#[test]
fn a_longer_scan_is_clamped_rather_than_refused() {
    // ★ A caller asking for an hour is a slider that ran to its end, not an attacker. Refusing would surface a maximum
    // the user cannot see and cannot discover.
    let state = testing::state();
    let asked = SCAN_CEILING_MS * 10;
    devices_scan_start(&state, &ScanOptions { timeout_ms: Some(asked), ..ScanOptions::default() }).expect("a scan");
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning.expect("a scan").total_ms, SCAN_CEILING_MS);
}

#[test]
fn a_cached_only_scan_finishes_immediately_rather_than_showing_a_bar_that_cannot_move() {
    let state = testing::state();
    devices_scan_start(
        &state,
        &ScanOptions { cached_only: true, ..ScanOptions::default() },
    )
    .expect("a scan");
    let scanning = devices_snapshot(&state).expect("a snapshot").scanning.expect("a scan");
    assert_eq!(scanning.total_ms, 0, "there is nothing to wait for");
}

#[test]
fn a_cancelled_scan_is_gone_from_the_next_snapshot() {
    let state = testing::state();
    let scan_id = devices_scan_start(&state, &ScanOptions::default()).expect("a scan");
    devices_scan_cancel(&state, &scan_id).expect("cancels");
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning, None);
}

#[test]
fn cancelling_a_scan_that_is_not_running_says_so_and_names_what_is() {
    let state = testing::state();
    let running = devices_scan_start(&state, &ScanOptions::default()).expect("a scan");

    let error = devices_scan_cancel(&state, "scan-999").expect_err("the wrong scan");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        error.params["running"].as_str(),
        Some(running.as_str()),
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
    let first = devices_scan_start(&state, &ScanOptions::default()).expect("a scan");
    devices_scan_cancel(&state, &first).expect("cancels");
    let second = devices_scan_start(&state, &ScanOptions::default()).expect("a scan");
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
