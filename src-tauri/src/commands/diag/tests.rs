//! Tests for the diagnostics command.
//!
//! # What these tests are really about
//!
//! A bundle is the one artefact a user hands to a stranger. Every test here is about the difference between what the
//! user was told and what is in the file — because that difference is the whole risk, and it is invisible once the file
//! has been sent.

use serde_json::Value;

use super::{DiagExportResult, diag_export};
use crate::testing;

fn options() -> synra_ipc::DiagExportOptions {
    synra_ipc::DiagExportOptions::default()
}

/// The bundle that was written, parsed back.
fn bundle(result: &DiagExportResult) -> Value {
    let text = std::fs::read_to_string(&result.path).expect("the bundle was written");
    serde_json::from_str(&text).expect("and it is JSON")
}

#[test]
fn a_bundle_is_written_where_it_says_it_is() {
    // ★ Because the path is the only part of the answer a user acts on.
    //
    // Returning a destination that does not exist produces a bundle that cannot be attached to a support ticket, and the
    // failure surfaces at the moment the user is least able to deal with it — after they have clicked "send".
    let root = testing::Root::fresh("diag-writes-where-it-says");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    assert!(std::path::Path::new(&result.path).is_file(), "the file it named is at {}", result.path);
}

#[test]
fn the_exclusions_are_written_into_the_file_not_only_returned() {
    // ★ The promise that is easy to get wrong.
    //
    // Returning the exclusion list to the screen is not enough: once the file is sent, the screen is gone. A bundle whose
    // own contents do not say what was left out relies on the recipient trusting whoever sent it.
    let root = testing::Root::fresh("diag-exclusions");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    let excluded = file.get("excluded").and_then(Value::as_array).expect("the file carries the list");
    assert!(!excluded.is_empty(), "and it is not empty — the defaults exclude the audit trail");
    assert_eq!(
        excluded,
        &result.excluded.iter().map(|section| Value::String(section.clone())).collect::<Vec<Value>>(),
        "what the screen showed and what the file says are the same list"
    );
}

/// Whether a key in a written bundle holds nothing.
///
/// A bundle's optional sections serialise as an explicit `null` rather than being absent, so "not included" has to
/// accept both spellings — and a test that only accepts one of them would pass for the wrong reason.
fn is_empty_section(file: &Value, key: &str) -> bool {
    matches!(file.get(key), None | Some(Value::Null))
}

#[test]
fn a_default_bundle_leaves_out_the_audit_trail_and_the_log() {
    // ★ Why the defaults are off.
    //
    // The audit trail names every plugin the user runs and the debug log is where a token that slipped through ends up.
    // Neither is included unless the user asked, and "asked" has to mean the option rather than the fact that a source
    // could be produced.
    let root = testing::Root::fresh("diag-defaults");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    assert!(is_empty_section(&file, "auditTrail"), "no audit trail without consent");
    assert!(is_empty_section(&file, "logs"), "and no log");
    assert!(
        !result.included.iter().any(|section| section == "audit-trail"),
        "and the consent screen said so before the file was written: {:?}",
        result.included
    );
}

#[test]
fn asking_for_the_audit_trail_puts_an_empty_one_in_rather_than_pretending_it_ran() {
    // ★ The choice, stated.
    //
    // There is no audit sink behind this shell, so the honest answer is "included and empty" — which the consent screen
    // can show — rather than omitting the section, which reads as "nothing happened".
    let root = testing::Root::fresh("diag-audit");
    let state = testing::state_in(&root);

    let result =
        diag_export(&state, &synra_ipc::DiagExportOptions { include_audit: true, ..options() }).expect("exports");
    let file = bundle(&result);
    assert_eq!(
        file.get("auditTrail").and_then(Value::as_array).map(Vec::len),
        Some(0),
        "present, and empty"
    );
    assert!(
        result.included.iter().any(|section| section == "audit-trail"),
        "and listed as included: {:?}",
        result.included
    );
}

#[test]
fn asking_for_the_debug_log_carries_the_section_too() {
    // ★ The other half of the consent switch.
    //
    // The audit test alone would still pass if only `include_audit` were wired up, which is exactly the shape a
    // half-finished consent flag takes.
    let root = testing::Root::fresh("diag-debug");
    let state = testing::state_in(&root);

    let result = diag_export(
        &state,
        &synra_ipc::DiagExportOptions { include_debug_logs: true, ..options() },
    )
    .expect("exports");
    assert!(
        result.included.iter().any(|section| section == "debug-logs"),
        "the debug log was asked for, so it is in the bundle: {:?}",
        result.included
    );
}

#[test]
fn the_exclusions_are_about_what_can_never_be_included_not_about_what_was_declined() {
    // ★ What `excluded` actually means, pinned because it is easy to assume otherwise.
    //
    // It is the standing policy list — clipboard, message bodies, file contents, passwords, a plugin's secure storage —
    // and it is the same whatever the user chose. Reading it as "what this user did not share" would make every bundle
    // look like it had declined something, and would hide the part that matters: none of it can ever be in one.
    let root = testing::Root::fresh("diag-exclusion-meaning");
    let state = testing::state_in(&root);

    let lean = diag_export(&state, &options()).expect("exports");
    let generous =
        diag_export(&state, &synra_ipc::DiagExportOptions { include_audit: true, ..options() }).expect("exports");

    assert_eq!(
        lean.excluded, generous.excluded,
        "consenting to more does not change the standing policy"
    );
    assert!(
        lean.excluded.iter().any(|section| section.contains("pairing secrets")),
        "and the policy does name what it is: {:?}",
        lean.excluded
    );
}

#[test]
fn the_configuration_in_the_bundle_is_what_the_settings_screen_shows() {
    let root = testing::Root::fresh("diag-config");
    let state = testing::state_in(&root);
    crate::commands::config_set(
        &state,
        &synra_ipc::ConfigPatch {
            set: [("ui.locale".to_owned(), serde_json::json!("zh"))].into_iter().collect(),
            unset: Vec::new(),
        },
    )
    .expect("writes a setting");

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    assert_eq!(
        file.get("config").and_then(|config| config.get("ui.locale")),
        Some(&serde_json::json!("zh")),
        "a maintainer reading this can see what the user had set"
    );
}

#[test]
fn the_bundle_names_the_versions_a_mismatch_would_be_found_in() {
    // ★ Both versions, and why both.
    //
    // A protocol mismatch between an app and a kernel is the single most common thing a bundle is sent for, and it is
    // invisible from either side alone.
    let root = testing::Root::fresh("diag-versions");
    let state = testing::state_in(&root);

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let environment = file.get("environment").expect("the environment is there");
    assert_eq!(environment.get("kernelVersion").and_then(Value::as_str), Some(crate::KERNEL_VERSION));
    assert_eq!(environment.get("appVersion").and_then(Value::as_str), Some(crate::KERNEL_VERSION));
    assert_eq!(environment.get("os").and_then(Value::as_str), Some(std::env::consts::OS));
}

#[test]
fn a_webview_version_is_absent_rather_than_guessed() {
    // ★ `None`, not a placeholder.
    //
    // This library does not touch a window, so it cannot know the WebView's version. Writing "unknown" would be a value,
    // and a maintainer would spend a day on it.
    let root = testing::Root::fresh("diag-webview");
    let state = testing::state_in(&root);

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let webview = file.get("environment").and_then(|environment| environment.get("webviewVersion"));
    assert!(webview.is_none() || webview == Some(&Value::Null), "absent or null, never invented");
}

#[test]
fn a_paired_device_appears_in_the_bundle_by_id() {
    let root = testing::Root::fresh("diag-devices");
    let state = testing::state_in(&root);
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: "44444444-4444-4444-8444-444444444444".to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            link_token: "secret-token-value".to_owned(),
            address: None,
            paired_at_ms: 1,
        })
        .expect("pairs");

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let devices = file.get("devices").and_then(Value::as_array).expect("the devices section is there");
    assert_eq!(devices.len(), 1);
    assert_eq!(
        devices[0].get("deviceId").and_then(Value::as_str),
        Some("44444444-4444-4444-8444-444444444444"),
        "so a maintainer can tell which machine was paired"
    );
}

#[test]
fn a_link_token_does_not_reach_the_bundle() {
    // ★ The test that matters most in this file.
    //
    // A `Paired` row carries a link token, which is the credential that lets anyone act as this device. A bundle is the
    // artefact most likely to be posted in a public issue tracker.
    let root = testing::Root::fresh("diag-token");
    let state = testing::state_in(&root);
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: "44444444-4444-4444-8444-444444444444".to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            link_token: "secret-token-value".to_owned(),
            address: Some("192.168.1.4:5000".to_owned()),
            paired_at_ms: 1,
        })
        .expect("pairs");

    let text = std::fs::read_to_string(diag_export(&state, &options()).expect("exports").path).expect("written");
    assert!(!text.contains("secret-token-value"), "the token is nowhere in the file");
}

#[test]
fn two_exports_in_one_session_are_two_files_and_not_one_overwritten() {
    // ★ The comparison this makes possible.
    //
    // Exporting twice is how a user shows "this started after I changed X" — so the two bundles have to both survive. A
    // name built from the second alone silently replaces the first, and the user compares a bundle against itself and
    // concludes nothing changed.
    let root = testing::Root::fresh("diag-twice");
    let state = testing::state_in(&root);

    let first = diag_export(&state, &options()).expect("first");
    let second = diag_export(&state, &options()).expect("second");

    assert_ne!(first.path, second.path, "the second export is not the first one again");
    assert!(std::path::Path::new(&first.path).is_file(), "and the first is still there");
    assert!(std::path::Path::new(&second.path).is_file());
}

#[test]
fn the_exclusions_are_the_same_whether_the_user_shared_more_or_less() {
    let root = testing::Root::fresh("diag-exclusion-constancy");
    let state = testing::state_in(&root);
    let first = diag_export(&state, &options()).expect("exports");
    let second =
        diag_export(&state, &synra_ipc::DiagExportOptions { include_audit: true, ..options() }).expect("exports");
    assert_eq!(first.excluded, second.excluded);
}
