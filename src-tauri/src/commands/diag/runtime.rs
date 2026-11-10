use super::*;

/// Reads the whole configuration, past anything unreadable.
pub(crate) fn configuration(state: &AppState) -> serde_json::Value {
    let rows = state.db().config().unwrap_or_default();
    let mut values = serde_json::Map::new();
    for row in rows {
        // ★ Kept as text when it will not parse, rather than dropped.
        //
        // The value is a redacted view of what is stored, and "this setting is unreadable" is itself the diagnostic. A
        // bundle that quietly omitted it would leave the maintainer guessing whether the setting was absent or broken.
        let value = serde_json::from_str(&row.value)
            .unwrap_or_else(|_| serde_json::Value::String(format!("<unreadable: {}>", row.value)));
        values.insert(row.key, value);
    }
    serde_json::Value::Object(values)
}

/// What this device is, as a maintainer needs to know it.
pub(crate) fn environment() -> EnvSummary {
    EnvSummary {
        app_version: crate::KERNEL_VERSION.to_owned(),
        kernel_version: crate::KERNEL_VERSION.to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        // ★ `None`, and not a guess.
        //
        // A WebView version is only knowable from inside the window, which this library deliberately does not touch — so
        // saying "unknown" would be a value, and a maintainer reading a bundle would spend time on it.
        webview_version: None,
    }
}

/// The path a bundle is written to.
///
/// # Why the identifier is a counter and not the clock
///
/// Because comparing a bundle from before a change with one from after it is the ordinary reason to export twice, and
/// two clicks inside one second is not unusual. A name built from seconds would put the second file on top of the first,
/// and the user would compare a bundle against itself and conclude nothing changed.
///
/// The start time is still carried, because a bundle from a process that started at 09:14 is worth knowing about even when
/// the counter restarts at 1 — a user comparing bundles from two runs needs to tell them apart.
pub(crate) fn destination(state: &AppState, bundle_id: u64) -> std::path::PathBuf {
    state
        .paths
        .data_dir()
        .join("diagnostics")
        .join(format!("bundle-{}-{bundle_id}.json", state.started_at_ms))
}

/// Reads the tail of the shell's own log.
///
/// # ★ Why this exists now and not before
///
/// Because there was nothing to read. Until the shell started its own log (`AppState::start_logging`), this command would
/// have read a path no code created — the same shape as the daemon's `synra logs`, which answered "no lines" for as long
/// as nothing wrote the file. Implementing it earlier would have shipped a command whose every answer was an empty list,
/// and an empty list is a claim: it says "nothing was logged" when the truth was "nothing is logging".
///
/// # Why it is not the same command as `logs`
///
/// `Request::Logs` in the control protocol is the **kernel's** log, read by the CLI from another process. This is the
/// **shell's** log, read in-process. They are two files because they are two processes, and a command that conflated
/// them would show a user the wrong one with no way to tell.
///
/// # Errors
///
/// [`IpcError::Internal`] only when the directory cannot be read at all. A missing log is not an error: a fresh
/// installation has not written one yet, and reporting that as a failure would make a working install look broken on its
/// first run.
pub fn diag_logs_tail(state: &AppState, lines: Option<u32>) -> Result<Vec<String>, IpcError> {
    let wanted = lines.map_or(synra_observe::logging::DEFAULT_TAIL_LINES, |n| n as usize);
    // ★ `verbose = true`, because this is a diagnostics command and the whole reason a user runs it is to see the detail
    // a quiet filter hides. It is also what `diag_export` does with the same file, so the two agree about what the log
    // contains — a pair of commands reading one file with different filters is how a user comes to distrust both.
    Ok(synra_observe::logging::tail(state.paths.log_dir(), wanted, true, None).lines)
}

/// The crash reports the next start would decide safe mode from, newest first.
///
/// # Why newest first
///
/// Because the prompt is about the run that just ended, and a list read top-down should start with it. `read_all` reads
/// the directory in whatever order the filesystem returns; sorting here is what makes the order a promise of the command
/// rather than an accident of the filesystem.
///
/// # Errors
///
/// Never in practice: an unreadable report is skipped by `read_all`, and a missing directory is an empty list.
pub fn diag_crashes(state: &AppState) -> Result<Vec<synra_observe::crash::CrashReport>, IpcError> {
    let mut reports = synra_observe::crash::CrashStore::at(state.paths.crash_dir()).read_all();
    reports.sort_by_key(|report| std::cmp::Reverse(report.at_ms));
    Ok(reports)
}

/// Deletes the crash reports that make the next start enter safe mode.
///
/// # Why "dismiss" deletes rather than remembers
///
/// Because the verdict is built from the reports on disk: a prompt that was answered "ignore" and left the file behind
/// would show the same prompt again next start, which is how a person learns to click through it without reading. The
/// reports are diagnostic material and are already included in an exported bundle, so the user who wants them keeps the
/// bundle; the one who says "ignore" gets a clean start.
///
/// # Errors
///
/// [`IpcError::Internal`] when a report exists but cannot be removed. A missing directory is not an error.
pub fn diag_crash_dismiss(state: &AppState) -> Result<(), IpcError> {
    let store = synra_observe::crash::CrashStore::at(state.paths.crash_dir());
    store.clear().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::Internal)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })
}

/// What the shell's metrics currently hold.
///
/// Why this command returns the snapshot rather than a rendering of it.
///
/// Because what a caller does with metrics is look for a name it cares about — `gate_denied_total` for a capability a plugin
/// keeps asking for and not getting — and a caller that has to parse a rendering is a caller that has to know how the
/// rendering was built. `MetricsSnapshot` is already the serialised shape the daemon puts in `status`, so both hosts answer
/// the same question the same way.
///
/// # Errors
///
/// Never in practice: the recorder is behind a lock it holds for the length of a read. It returns a `Result` because every
/// command in this module does, and a command that cannot fail is not a reason to break the shape the interface expects.
// The CPU percentage is a float from `sysinfo` and the recorder's gauge is an integer: rounding it loses fractions of a
// percent, which is the only precision a "how busy is this process" gauge can use anyway.
#[allow(clippy::cast_possible_truncation)]
pub fn diag_metrics(state: &AppState) -> Result<synra_observe::metrics::MetricsSnapshot, IpcError> {
    // ★ Sampled at read rather than written by the store: the file grows while the process runs, and no single write
    // point knows the final size. A missing file is zero — an in-memory database, or a database not yet created.
    let size = std::fs::metadata(state.paths.db_path()).map_or(0, |metadata| metadata.len());
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("db_size_bytes", []),
        i64::try_from(size).unwrap_or(i64::MAX),
    );
    // ★ Uptime is sampled here for the same reason the sizes are: nothing else knows when the shell started, and a
    // counter advanced by the delta keeps the series monotonic across every read.
    state.record_uptime(state.uptime().as_secs());
    // ★ The process gauges come from `sysinfo`, kept on `AppState` between reads because CPU is a delta. `None` means the
    // platform would not describe the process; no gauges are written then, rather than a zero standing in for "unknown".
    if let Some(process) = state.sample_process() {
        state.metrics.gauge_set(
            &synra_observe::metrics::SeriesKey::new("memory_rss_bytes", []),
            i64::try_from(process.memory_bytes).unwrap_or(i64::MAX),
        );
        state.metrics.gauge_set(
            &synra_observe::metrics::SeriesKey::new("cpu_percent", []),
            process.cpu_percent.round() as i64,
        );
        if let Some(threads) = process.threads {
            state.metrics.gauge_set(
                &synra_observe::metrics::SeriesKey::new("threads", []),
                i64::try_from(threads).unwrap_or(i64::MAX),
            );
        }
    }
    // ★ The log's own counters, sampled where the write path can be observed rather than in the log layer (which must not
    // depend on metrics). Without these the shell's diagnostics could say nothing about a logger that is dropping lines,
    // which is the one failure that looks like "nothing happened".
    if let Some(guard) = state.log.as_ref() {
        let stats = synra_observe::logging::stats_of(guard);
        for (name, value) in [
            ("log_lines", stats.lines()),
            ("log_dropped", stats.dropped()),
            ("log_write_errors", stats.write_errors()),
        ] {
            state.metrics.gauge_set(
                &synra_observe::metrics::SeriesKey::new(name, []),
                i64::try_from(value).unwrap_or(i64::MAX),
            );
        }
    }
    // ★ How many late answers are waiting for a delivery attempt. A queue that keeps growing is a device that is gone and a
    // shell that keeps producing answers for it; the number is the only place that is visible without reading the directory.
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("late_results", []),
        i64::try_from(synra_core::late_queue::LateQueue::in_data_dir(state.paths.data_dir()).len())
            .unwrap_or(i64::MAX),
    );
    Ok(state.metrics.snapshot())
}

/// The one place a database or filesystem failure becomes an `IpcError`.
pub(crate) fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::Internal)
        .with_params(serde_json::json!({ "reason": error.to_string() }))
}
