use super::*;

#[test]
fn a_fresh_shell_reports_what_building_it_measured_and_nothing_else() {
    // """ + S + """ The module's rule, on the command that had the least of it: a command either answers or says why it cannot,
    // and never invents an answer.
    //
    // A shell that has done nothing but start has only the measurements of starting (its own construction time and the
    // writes that construction makes) — and that is a different fact from "this build cannot tell you", which the
    // interface renders separately. So the answer is `Ok` with those rows, not an error a caller must special-case.
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
    for expected in [
        "db_size_bytes",
        "uptime_seconds",
        "memory_rss_bytes",
        "cpu_percent",
    ] {
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
        // ★ Building the state is itself a measurement now: `startup_duration_ms{phase=state}` is the cold-start number,
        // so a fresh shell has this row before any command has run. It is a real observation, not an invented sample.
        "startup_duration_ms",
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
    assert!(
        !names.contains(&"log_lines"),
        "a log gauge was invented without a log: {value}"
    );
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
        labels
            .iter()
            .any(|pair| pair[0] == "capability" && pair[1] == "device:send:*"),
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

    assert_eq!(
        snapshot.get("db_size_bytes"),
        Some(&synra_observe::metrics::Value::Gauge(32))
    );
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

    let names: Vec<&str> = report
        .results
        .iter()
        .map(|result| result.name.as_str())
        .collect();
    for expected in ["tcp_port_available", "data_dir_writable", "webview_version"] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
    assert!(
        report
            .results
            .iter()
            .all(|result| ["ok", "warn", "fail"].contains(&result.status.as_str())),
        "a status outside the contract's three words"
    );
}

#[test]
fn dismissing_crashes_clears_the_reports_that_would_enter_safe_mode() {
    // ★ "Ignore" has to be durable: a prompt that leaves the report behind shows itself again next start, which is how
    // a person learns to click through it. This is the button that turns a fixed installation back into a normal one.
    let state = testing::state();
    let store = synra_observe::crash::CrashStore::at(state.paths.crash_dir());
    for offset in 0..synra_observe::crash::SAFE_MODE_THRESHOLD {
        store
            .write(&synra_observe::crash::CrashReport::new(
                1_000 + i64::try_from(offset).expect("a small offset"),
                "0.1.0",
                Some("src/plugin.rs:42".to_owned()),
                Some("the plugin panicked".to_owned()),
                None,
                None,
                None,
            ))
            .expect("the report is written");
    }
    assert!(
        !store.verdict(1_000).allows_plugins(),
        "the fixture did not reach safe mode"
    );

    super::super::diag_crash_dismiss(&state).expect("the reports are dismissed");

    assert!(store.is_empty(), "a report survived the dismiss");
    assert!(
        store.verdict(1_000).allows_plugins(),
        "the next start would still be in safe mode"
    );
}

#[test]
fn the_crash_list_is_newest_first_because_the_prompt_is_about_the_last_run() {
    // ★ The filesystem's order is not a promise; the command's order is. Two reports written oldest-first must come back
    // newest-first, or the list a person reads starts at the wrong end of the story.
    // ★ A directory of this test's own. `testing::state()` sits on one shared scratch path, so the dismiss test's
    // reports were visible here and the list came back four long instead of two.
    let root = testing::Root::fresh("crash-list");
    let state = testing::state_in(&root);
    let store = synra_observe::crash::CrashStore::at(state.paths.crash_dir());
    for at_ms in [1_000, 2_000] {
        store
            .write(&synra_observe::crash::CrashReport::new(
                at_ms,
                "0.1.0",
                Some(format!("src/plugin.rs:{at_ms}")),
                Some("the plugin panicked".to_owned()),
                None,
                None,
                None,
            ))
            .expect("the report is written");
    }

    let reports = super::super::diag_crashes(&state).expect("the list answers");

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].at_ms, 2_000, "the newest report is not first");
    assert_eq!(reports[1].at_ms, 1_000);
}
