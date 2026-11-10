use super::*;

#[test]
fn a_network_scan_records_itself_while_it_runs_and_clears_that_before_it_answers() {
    // ★ The state's own meaning, asserted from outside the command: `scanning` is *the scan in progress*. That is what the
    // interface reads to disable the button and draw the bar, so a scan that records nothing looks like a dead button, and
    // one that never clears looks like a button that has died.
    let (state, deadline) = with_discovery();
    let subscriber = state.events.subscribe();
    let scan_id = super::super::scan_with(
        &state,
        &ScanOptions {
            timeout_ms: Some(60),
            ..ScanOptions::default()
        },
        |discovery, budget, cancellation| {
            // The production path publishes its deadline before this wait; inspecting here cannot miss a short scan.
            discovery.scan_with_wait(budget, 1_000, cancellation, |window| {
                assert_eq!(window, Duration::from_millis(60));
                let snapshot = devices_snapshot(&state).expect("running snapshot");
                assert_eq!(snapshot.scanning.expect("scan in progress").total_ms, 60);
                assert_eq!(deadline.load(Ordering::Relaxed), 1_060);
            });
        },
    )
    .expect("a scan with a worker behind it");
    assert!(
        !scan_id.is_empty(),
        "the contract answers with an identifier"
    );
    assert_eq!(
        devices_snapshot(&state).expect("a snapshot").scanning,
        None,
        "and it is over when it answers"
    );
    assert_eq!(
        deadline.load(Ordering::Relaxed),
        0,
        "with the deadline released, so nothing keeps broadcasting"
    );
    let delivery = state.events.take(subscriber).expect("start and completion");
    assert_eq!(delivery.events().len(), 2);
    for event in delivery.events() {
        assert!(
            matches!(event, synra_ipc::event::KernelEvent::ScanUpdated(scan) if scan.scan_id == scan_id)
        );
    }

    // ★ The scan is measured where it runs. An empty test network found nothing, and "found nothing" is still an
    // observation — a histogram with no observations would be a shape rather than a measurement.
    use synra_observe::metrics::Value;
    let snapshot = state.metrics.snapshot();
    assert_eq!(
        snapshot.get("scans_total{result=ran}"),
        Some(&Value::Counter(1))
    );
    match snapshot.get("candidates_found") {
        Some(Value::Histogram(histogram)) => assert_eq!(histogram.count, 1),
        other => panic!("expected a candidates histogram, got {other:?}"),
    }
    assert!(
        snapshot.get("scan_duration_ms").is_some(),
        "the scan's time is recorded"
    );
}

#[test]
fn a_real_scan_can_be_cancelled_from_another_command_and_publishes_completion() {
    let (state, deadline) = with_discovery();
    let subscriber = state.events.subscribe();
    std::thread::scope(|scope| {
        let scanning = scope.spawn(|| devices_scan_start(&state, &ScanOptions::default()));
        let limit = std::time::Instant::now() + Duration::from_secs(5);
        while deadline.load(Ordering::Relaxed) == 0 {
            assert!(std::time::Instant::now() < limit, "scan never began");
            std::thread::yield_now();
        }
        let id = devices_snapshot(&state)
            .expect("snapshot")
            .scanning
            .expect("active scan")
            .scan_id;
        devices_scan_cancel(&state, &id).expect("cancel the real wait");
        assert_eq!(scanning.join().expect("worker").expect("scan result"), id);
    });
    assert_eq!(deadline.load(Ordering::Relaxed), 0);
    assert!(
        devices_snapshot(&state)
            .expect("snapshot")
            .scanning
            .is_none()
    );
    let events = state.events.take(subscriber).expect("scan events");
    assert_eq!(events.events().len(), 2);
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("scans_total{result=cancelled}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
}

#[test]
fn a_worker_unwind_releases_both_the_demand_and_the_task() {
    let (state, deadline) = with_discovery();
    let subscriber = state.events.subscribe();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = super::super::scan_with(
            &state,
            &ScanOptions::default(),
            |discovery, budget, cancellation| {
                discovery.scan_with_wait(budget, 1_000, cancellation, |_| {
                    panic!("injected scan failure")
                });
            },
        );
    }));
    assert!(result.is_err());
    assert_eq!(deadline.load(Ordering::Relaxed), 0);
    assert!(
        devices_snapshot(&state)
            .expect("snapshot")
            .scanning
            .is_none()
    );
    assert_eq!(
        state
            .events
            .take(subscriber)
            .expect("completion after failure")
            .events()
            .len(),
        2
    );
    assert_eq!(
        state.metrics.snapshot().get("scans_total{result=failed}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
}

#[test]
fn a_cached_only_scan_finishes_immediately_rather_than_showing_a_bar_that_cannot_move() {
    let state = testing::state();

    let scan_id = devices_scan_start(&state, &cached()).expect("a scan");
    assert!(
        !scan_id.is_empty(),
        "the contract answers with an identifier"
    );

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

    let parsed: ScanOptions =
        serde_json::from_value(sent).expect("the interface's payload is this struct");
    assert!(
        parsed.cached_only,
        "the interface asks for the scan this shell can answer"
    );
    assert!(!parsed.include_paired);
    assert_eq!(parsed.timeout_ms, None);
}
