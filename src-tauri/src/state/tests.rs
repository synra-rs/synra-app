use synra_observe::metrics::Value;

#[test]
fn constructing_the_state_records_how_long_it_took() {
    // ★ M4 asks for a cold start under 500 ms. A budget that is never written down on the machine is a sentence, not a
    // test; this is the number the performance panel reads.
    let state = crate::testing::state();

    assert!(
        state
            .metrics
            .snapshot()
            .get("startup_duration_ms{phase=state}")
            .is_some(),
        "the state's own construction time is not recorded"
    );
}

#[test]
fn the_shells_host_counts_into_the_shells_recorder() {
    // ★ The host had no recorder before this: `Host::with_metrics` existed for the daemon, and the window built its
    // host without one — so the frame and link counters were zero in the only host a desktop user runs. The counter
    // is public precisely so a host can be shown to have one; the assertion is on the recorder the rest of the shell
    // reads (`diag_metrics`), not on a private field.
    let state = crate::testing::state();

    state.host.lock().count_frame_sent("kernel.test");

    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("frames_sent_total{event=kernel.test}"),
        Some(&Value::Counter(1)),
        "the host's frame counter is the shell's recorder"
    );
}

#[test]
fn a_store_write_reaches_the_shells_recorder() {
    // ★ The store has no recorder of its own: it reports a write to the host that installed an observer. This is the
    // shell's install, checked through the recorder `diag_metrics` reads — a write the user waits on should be in the
    // sample the performance panel shows.
    let state = crate::testing::state();

    state
        .db()
        .set_config("test.key", &serde_json::json!(1), 1)
        .expect("the write is made");

    assert!(
        state
            .metrics
            .snapshot()
            .get("db_write_duration_us{kind=config}")
            .is_some(),
        "the store's write was not counted by the shell's recorder"
    );
}

#[test]
fn uptime_is_added_as_a_delta_rather_than_the_whole_value_again() {
    // ★ A counter set to the whole uptime on every read would grow with the number of refreshes, not with time. The
    // second sample says "four more seconds have passed", so the total is 14, not 24 — an assertion that only checked
    // one read would pass whichever way the arithmetic went.
    let state = crate::testing::state();

    state.record_uptime(10);
    state.record_uptime(14);

    assert_eq!(
        state.metrics.snapshot().get("uptime_seconds"),
        Some(&Value::Counter(14))
    );
}
