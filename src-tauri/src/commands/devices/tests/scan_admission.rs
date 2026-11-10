use super::*;

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
    assert!(
        state.discovery.is_none(),
        "the fixture is a shell that could not bind the sockets"
    );

    let refused =
        devices_scan_start(&state, &ScanOptions::default()).expect_err("this state cannot scan");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    let reason = refused
        .params
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        reason.contains("discovery sockets"),
        "the refusal names what is missing: {reason}"
    );
    // And nothing was left in the snapshot claiming a scan is under way.
    assert_eq!(devices_snapshot(&state).expect("a snapshot").scanning, None);
    // ★ "Why can this device not find anything" is a question the metrics answer: the refusal is a scan that could not run,
    // and the daemon writes the same label for its own refusal.
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("scans_total{result=unavailable}"),
        Some(&synra_observe::metrics::Value::Counter(1)),
        "the refusal is counted rather than only returned"
    );
}

#[test]
fn a_scan_budget_is_the_protocols_and_not_the_callers() {
    // A scan is a broadcast to every machine on the network, so its length is the protocol's to set. The two interesting
    // answers are the ceiling — a caller does not get to ask for a longer broadcast — and the default, which is the
    // window "look now" means when nobody said how long.
    let asked = |ms: Option<u64>| {
        scan_budget(&ScanOptions {
            timeout_ms: ms,
            ..ScanOptions::default()
        })
    };

    assert_eq!(asked(None), synra_protocol::limits::SCAN_PASSIVE);
    assert_eq!(
        asked(Some(10)),
        Duration::from_millis(10),
        "a shorter scan is honoured"
    );
    assert_eq!(
        asked(Some(60_000)),
        synra_protocol::limits::SCAN_TOTAL,
        "a longer one is clamped"
    );
    assert!(
        asked(Some(0)).is_zero(),
        "and zero is the protocol's way of saying `answer from what you know`"
    );
}

#[test]
fn overlapping_network_scans_are_refused_until_the_owner_exits_even_after_cancel() {
    let (state, deadline) = with_discovery();
    super::super::scan_with(
        &state,
        &ScanOptions::default(),
        |discovery, budget, cancellation| {
            discovery.scan_with_wait(budget, 1_000, cancellation, |_| {
                let id = devices_snapshot(&state)
                    .expect("snapshot")
                    .scanning
                    .expect("active")
                    .scan_id;
                devices_scan_start(&state, &cached()).expect("cache does not need the scanner");
                assert!(deadline.load(Ordering::Relaxed) > 0);
                devices_scan_cancel(&state, &id).expect("request accepted");
                assert!(cancellation.is_cancelled());
                let refused = devices_scan_start(&state, &ScanOptions::default())
                    .expect_err("the old owner has not exited");
                assert_eq!(refused.code, ErrorCode::RateLimited);
                assert_eq!(refused.params["running"].as_str(), Some(id.as_str()));
                assert_eq!(
                    devices_snapshot(&state)
                        .expect("snapshot")
                        .scanning
                        .expect("still active")
                        .scan_id,
                    id
                );
            });
        },
    )
    .expect("first scan");
    assert_eq!(deadline.load(Ordering::Relaxed), 0);
    assert!(
        devices_snapshot(&state)
            .expect("snapshot")
            .scanning
            .is_none()
    );
    super::super::scan_with(&state, &ScanOptions::default(), |_, _, _| {})
        .expect("the next scan is admitted");
}

#[test]
fn cancellation_requests_a_stop_without_pretending_the_worker_has_exited() {
    let state = testing::state();
    scanning(&state, "scan-1");
    devices_scan_cancel(&state, "scan-1").expect("cancels");
    assert!(
        state
            .scanning
            .lock()
            .as_ref()
            .expect("still owned")
            .cancellation
            .is_cancelled()
    );
    assert_eq!(
        devices_snapshot(&state)
            .expect("a snapshot")
            .scanning
            .expect("still running")
            .scan_id,
        "scan-1"
    );
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
