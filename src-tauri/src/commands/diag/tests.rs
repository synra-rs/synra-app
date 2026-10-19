//! Tests for the diagnostics command.
//!
//! # What these tests are really about
//!
//! A bundle is the one artefact a user hands to a stranger. Every test here is about the difference between what the
//! user was told and what is in the file — because that difference is the whole risk, and it is invisible once the file
//! has been sent.

use serde_json::Value;

use super::{DiagExportResult, diag_export, diag_metrics, diag_probes};
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
    // ★ The choice, stated: the section is present rather than omitted.
    //
    // A missing section and an empty one read the same on a screen, and the first is indistinguishable from "this bundle
    // came from a version that never had one". So the section is always there when it was asked for.
    //
    // ★ What this test does **not** cover, said plainly rather than implied.
    //
    // It cannot tell a section wired to the plugin set's ring from one still stubbed to `Some(Vec::new())` — both are
    // empty here, because this fixture has no plugin engine, so no instance ran and no call was authorised. Catching that
    // needs an audit entry, and an entry is produced when a **guest makes a host call**: the scripted backend never
    // enters a guest, so no fixture in this repository can produce one. The same missing toolchain (`wasm32-wasip2`)
    // that leaves `synra-plugin-host`'s two guest tests `#[ignore]`d leaves this seam unguarded.
    //
    // The binding is a one-line read of `state.plugin_set()`, and this comment is where the next person finds out that
    // nothing would go red if it were removed.
    let root = testing::Root::fresh("diag-audit");
    let state = testing::state_in(&root);

    let result =
        diag_export(&state, &synra_ipc::DiagExportOptions { include_audit: true, ..options() }).expect("exports");
    let file = bundle(&result);
    assert_eq!(
        file.get("auditTrail").and_then(Value::as_array).map(Vec::len),
        Some(0),
        "present, and empty because nothing has run"
    );
    assert!(
        result.included.iter().any(|section| section == "audit-trail"),
        "and listed as included: {:?}",
        result.included
    );

    // And absent when it was not asked for, which is the half this test *can* check.
    let result = diag_export(&state, &synra_ipc::DiagExportOptions { include_audit: false, ..options() })
        .expect("exports");
    assert!(
        !result.included.iter().any(|section| section == "audit-trail"),
        "not asked for, so not in the bundle: {:?}",
        result.included
    );
}

#[test]
fn the_plugin_section_reports_what_the_set_knows_rather_than_a_constant() {
    // ★ The three fields that used to be constants.
    //
    // `had_refusals: false`, `active: false` and `version: String::new()` were written into every bundle, and the first
    // of them is a **claim**: it tells a maintainer nothing was ever refused, in the document a user sends when they are
    // asking why something did not work.
    //
    // ★ What this covers and what it does not. The fixture has no plugin engine, so the set answers "not loaded, no
    // version, nothing refused" for every installed plugin — which is the truth about a shell with no engine. So this
    // asserts the *wiring* in one direction: a bundle made here says `active: false` because nothing is loaded, not
    // because the field is a literal. It cannot tell the two apart, and that seam is recorded where the read happens
    // (`loaded_plugins`), for the same reason the audit one is: producing a loaded plugin needs a guest that calls a host
    // function, and there is no toolchain for that in this repository.
    let root = testing::Root::fresh("diag-plugins");
    let state = testing::state_in(&root);
    // Installed so the section has a row to describe; a bundle with no plugins in it would make the assertions vacuous.
    state
        .db()
        .insert_plugin(
            "@acme/notes",
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    let plugins = file.get("plugins").and_then(Value::as_array).expect("a plugin section");

    // The fixture installs plugins, and each one is reported — rather than the section being empty, which would make the
    // assertions below vacuous.
    assert_eq!(plugins.len(), 1, "the fixture installs one plugin: {file}");
    for plugin in plugins {
        assert_eq!(plugin.get("active").and_then(Value::as_bool), Some(false));
        assert_eq!(plugin.get("hadRefusals").and_then(Value::as_bool), Some(false));
        assert!(plugin.get("version").is_some(), "the key is present even when the value is empty");
    }
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
fn the_log_tail_reads_the_shells_own_log_including_the_debug_lines() {
    // ★ The command was unbuildable before the shell had a log, and this is the test that says it now reads one.
    //
    // Two things are asserted and they are different: that the lines come back at all, and that the **debug** ones do.
    // A quiet filter would return only the `INFO` line, and a caller would conclude the log had nothing useful in it —
    // which is the opposite of why they ran a diagnostics command.
    let root = testing::Root::fresh("diag-log-tail");
    let state = testing::state_in(&root);

    let log_dir = state.paths.log_dir();
    std::fs::create_dir_all(log_dir).expect("the log directory");
    std::fs::write(
        log_dir.join("synra.log"),
        "2026-01-01T00:00:00Z INFO first\n\
         2026-01-01T00:00:01Z INFO second\n\
         2026-01-01T00:00:02Z DEBUG third\n",
    )
    .expect("a log");

    let all = super::diag_logs_tail(&state, None).expect("reads");
    assert_eq!(all.len(), 3, "every line, debug included: {all:?}");
    assert!(all.iter().any(|line| line.contains("third")), "the debug line is missing: {all:?}");

    // The count is honoured, and the tail is the newest end — a user asking for the last line wants the last line, not
    // the first.
    let last = super::diag_logs_tail(&state, Some(1)).expect("reads");
    assert_eq!(last.len(), 1);
    assert!(last[0].contains("third"), "{last:?}");
}

#[test]
fn the_log_tail_is_empty_rather_than_an_error_when_nothing_has_been_written() {
    // A fresh installation has not logged anything yet. Reporting that as a failure would make a working install look
    // broken the first time a user opened diagnostics.
    let root = testing::Root::fresh("diag-log-tail-empty");
    let state = testing::state_in(&root);

    let lines = super::diag_logs_tail(&state, None).expect("an absent log is not a failure");
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn the_debug_log_section_carries_the_lines_the_shell_wrote() {
    // ★ The assertion the test above cannot make, and the reason this one exists.
    //
    // `asking_for_the_debug_log_carries_the_section_too` checks that the section is *listed*. It passed for as long as the
    // source was `Some(Vec::new())` — an included section holding nothing, which is the shape a half-built feature takes:
    // the consent screen agrees with the file about a section that has no contents in it. A user who shares that bundle
    // believes they have sent their log.
    //
    // So this test writes a real log where the shell keeps it and requires the lines to come back out of the file.
    let root = testing::Root::fresh("diag-debug-lines");
    let state = testing::state_in(&root);

    let log_dir = state.paths.log_dir();
    std::fs::create_dir_all(log_dir).expect("the log directory");
    std::fs::write(
        log_dir.join("synra.log"),
        "2026-01-01T00:00:00Z INFO the shell started\n\
         2026-01-01T00:00:01Z DEBUG something only a debug log has\n",
    )
    .expect("a log");

    let result = diag_export(
        &state,
        &synra_ipc::DiagExportOptions {
            include_debug_logs: true,
            log_lines: Some(50),
            ..options()
        },
    )
    .expect("exports");

    let file = bundle(&result);
    let text = serde_json::to_string(&file).expect("the bundle renders");
    assert!(
        text.contains("the shell started"),
        "the log the shell wrote is not in the bundle it produced: {text}"
    );
    // ★ And the debug line too, because the user asked for debug logs. A quiet filter here would hide exactly what was
    // requested, and the bundle would carry a section that looked deliberate and was empty.
    assert!(
        text.contains("something only a debug log has"),
        "the debug lines were filtered out of the debug-log section: {text}"
    );
}

#[test]
fn the_debug_log_section_is_empty_rather_than_absent_when_there_is_no_log() {
    // The first run of a fresh installation. The distinction is the one the audit-trail comment draws: an included and
    // empty section says "nothing was recorded", where an absent one is indistinguishable from a version that never had
    // the feature. Both are honest here, and the user consented to the section, so it is included.
    let root = testing::Root::fresh("diag-no-log");
    let state = testing::state_in(&root);

    let result = diag_export(
        &state,
        &synra_ipc::DiagExportOptions { include_debug_logs: true, ..options() },
    )
    .expect("exports");

    assert!(
        result.included.iter().any(|section| section == "debug-logs"),
        "a section the user asked for is listed even when it has nothing in it: {:?}",
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

#[test]
fn an_empty_shell_reports_no_metrics_rather_than_refusing() {
    // """ + S + """ The module's rule, on the command that had the least of it: a command either answers or says why it cannot,
    // and never invents an answer.
    //
    // A shell that has not yet refused anything has **no** metrics, which is a different fact from "this build cannot tell
    // you" — and the interface renders those two differently. So an empty snapshot is the answer, and `Ok` is right: the
    // caller gets a list with nothing in it rather than an error it would have to special-case.
    let root = testing::Root::fresh("diag-metrics-empty");
    let state = testing::state_in(&root);

    let snapshot = diag_metrics(&state).expect("metrics");
    let value = serde_json::to_value(&snapshot).expect("serialises");

    // ★ The command answers with the samples a shell can have before it has done anything: the database size, the
    // uptime, and the process facts — all four exist from the first migration and the first clock tick. The rule under
    // test — answer rather than refuse, and invent nothing — is unchanged; the assertion now says what the answer may
    // hold, and `threads` is allowed to be absent because not every platform reports it.
    let values = value["values"].as_array().expect("the sample list");
    let names: Vec<&str> = values
        .iter()
        .filter_map(|sample| sample["key"]["name"].as_str())
        .collect();
    for expected in ["db_size_bytes", "uptime_seconds", "memory_rss_bytes", "cpu_percent"] {
        assert!(names.contains(&expected), "{value}");
    }
    // ★ `db_write_duration_us` is allowed from the first tick now: the shell installs the store's write observer while
    // building this state, and building it **writes** (the identity row, and the audit row that the fixture's plugin
    // history leaves). That is a real write in this process, not a test inventing a sample — the rule under test is that
    // nothing is counted that did not happen, and these happened.
    let sampled = [
        "db_size_bytes",
        "uptime_seconds",
        "memory_rss_bytes",
        "cpu_percent",
        "threads",
        "db_write_duration_us",
        "late_results",
    ];
    assert!(
        names.iter().all(|name| sampled.contains(name)),
        "something was counted before anything ran: {value}"
    );
    // ★ The queue gauge is a reading of a directory, not a count of something that happened, and an empty shell has an
    // empty queue: zero is the truthful sample.
    assert_eq!(
        snapshot.get("late_results"),
        Some(&synra_observe::metrics::Value::Gauge(0)),
        "{value}"
    );
    // ★ A shell whose logger is not running writes no log gauges at all: zero would be "running and quiet", which is a
    // different fact from "not running".
    assert!(!names.contains(&"log_lines"), "a log gauge was invented without a log: {value}");
}

#[test]
fn what_the_gate_counts_is_what_the_command_reports() {
    // """ + S + """ The join, and the reason this command exists at all.
    //
    // `diag_metrics` used to be declared by the contract and implemented nowhere, with the reason recorded in the
    // registration test: *"no metrics sink is behind `AppState`"*. Adding a sink is only half of it — the other half is that
    // the recorder the **gate** writes into and the recorder the **command** reads are the same one. A command over a
    // second, empty recorder would answer `Ok` with nothing in it forever, which is the most misleading shape a diagnostic
    // can have.
    //
    // So this writes a count the way the gate writes one and asserts the command sees it, labels and all — because a count
    // without its label cannot answer "which permission do plugins keep asking for and not have".
    let root = testing::Root::fresh("diag-metrics-join");
    let state = testing::state_in(&root);

    let key = synra_observe::metrics::SeriesKey::new(
        "gate_denied_total",
        [("capability".to_owned(), "device:send:*".to_owned())],
    );
    state.metrics.counter_inc(&key);

    let value = serde_json::to_value(diag_metrics(&state).expect("metrics")).expect("serialises");
    let series = value["values"].as_array().expect("the series list");
    let found = series
        .iter()
        .find(|entry| entry["key"]["name"] == "gate_denied_total")
        .expect("the count the gate made is reported");

    assert_eq!(found["value"]["value"], 1);
    let labels = found["key"]["labels"].as_array().expect("labels");
    assert!(
        labels.iter().any(|pair| pair[0] == "capability" && pair[1] == "device:send:*"),
        "{found}"
    );
}

#[test]
fn the_metrics_answer_includes_the_database_size() {
    // ★ The design's `db_size_bytes` gauge, sampled where the metrics are read: no write point knows the final size, and
    // the file grows while the process runs. The test writes a file where the database would be, so the number is a known
    // one rather than whatever a timing-dependent database happened to hold.
    let state = testing::state();
    let path = state.paths.db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the data directory");
    }
    std::fs::write(&path, [0u8; 32]).expect("a database-sized file");

    let snapshot = diag_metrics(&state).expect("metrics");

    assert_eq!(snapshot.get("db_size_bytes"), Some(&synra_observe::metrics::Value::Gauge(32)));
}

#[test]
fn the_metrics_answer_carries_the_process_memory() {
    // ★ The platform readout lands in the recorder, not in a private field: a process that is running has more than zero
    // resident bytes, so the assertion is that a number arrived rather than the exact number (which no test can know).
    let state = testing::state();

    let snapshot = diag_metrics(&state).expect("metrics");

    match snapshot.get("memory_rss_bytes") {
        Some(synra_observe::metrics::Value::Gauge(bytes)) => {
            assert!(*bytes > 0, "a running process reports some resident memory");
        }
        other => panic!("expected a memory gauge, got {other:?}"),
    }
}

#[test]
fn the_window_asks_the_same_probe_questions_the_cli_does() {
    // ★ The statuses depend on the machine running the test, so they are not asserted. What is assertable is that the
    // window asks the same questions: a report missing `tcp_port_available` would be a different diagnostic from the one
    // `synra doctor` prints, and the two are supposed to be the same evidence.
    let state = testing::state();

    let report = diag_probes(&state).expect("the probes run");

    let names: Vec<&str> = report.results.iter().map(|result| result.name.as_str()).collect();
    for expected in ["tcp_port_available", "data_dir_writable", "webview_version"] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
    assert!(
        report.results.iter().all(|result| ["ok", "warn", "fail"].contains(&result.status.as_str())),
        "a status outside the contract's three words"
    );
}
