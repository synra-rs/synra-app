/// Records one inbound attempt, as the receiving half saw it.
///
/// # Why an aborted transfer records no size
///
/// For the same reason a cancelled send does not: the size histogram describes files that arrived. The attempt is still
/// counted (`outcome=aborted`), because "someone sent and it did not land" is the fact the user is asking about.
///
/// # Why the elapsed time is optional
///
/// The listener learns a transfer started from its first progress event. An abort that arrives before that event has no
/// start to subtract, and inventing a zero would put a transfer that never moved into the latency distribution.
///
/// ★ The only caller is the listener, which is compiled only into the shell build (`tauri-shell`). The no-default build
/// still has this function because its unit test runs there, so it is dead code **in that build only** — allowed here
/// rather than hidden behind a `cfg(test)` that would make the function untestable from the shell's own module.
#[cfg_attr(not(feature = "tauri-shell"), allow(dead_code))]
pub(crate) fn record_received(
    state: &crate::state::AppState,
    kind: &str,
    byte_length: u64,
    elapsed: Option<std::time::Duration>,
    aborted: bool,
) {
    record_attempt(
        state,
        kind,
        byte_length,
        elapsed,
        if aborted { "aborted" } else { "received" },
        !aborted,
    );
}

/// The shape both directions share.
///
/// # Why the names are the daemon's
///
/// The daemon writes `transfer_bytes{kind}` and `transfer_duration_ms{kind}`, so the same transfer must not be called
/// something else by the other host. `transfers_total` adds `outcome` because the daemon deliberately left it out — its
/// exits are not funnelled, and a counter written on the success path alone would answer the wrong question.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn record_attempt(
    state: &crate::state::AppState,
    kind: &str,
    byte_length: u64,
    elapsed: Option<std::time::Duration>,
    outcome: &str,
    record_bytes: bool,
) {
    state
        .metrics
        .counter_inc(&synra_observe::metrics::SeriesKey::new(
            "transfers_total",
            [
                ("kind".to_owned(), kind.to_owned()),
                ("outcome".to_owned(), outcome.to_owned()),
            ],
        ));
    if let Some(elapsed) = elapsed {
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "transfer_duration_ms",
                [("kind".to_owned(), kind.to_owned())],
            ),
            elapsed.as_secs_f64() * 1_000.0,
        );
    }
    if record_bytes {
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "transfer_bytes",
                [("kind".to_owned(), kind.to_owned())],
            ),
            byte_length as f64,
        );
        // ★ Throughput only for a transfer that moved its bytes, and only when there is a duration to divide by. The
        // window is the whole attempt (for a send, including the dial), so this is end-to-end throughput rather than the
        // link's line rate — which is the number a user comparing "how long did this take" is asking about.
        if let Some(elapsed) = elapsed {
            let seconds = elapsed.as_secs_f64();
            if seconds > 0.0 {
                state.metrics.histogram_observe(
                    &synra_observe::metrics::SeriesKey::new(
                        "transfer_throughput_bps",
                        [("kind".to_owned(), kind.to_owned())],
                    ),
                    byte_length as f64 / seconds,
                );
            }
        }
    }
}
