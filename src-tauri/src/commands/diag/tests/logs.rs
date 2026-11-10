use super::*;

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
        &synra_ipc::DiagExportOptions {
            include_debug_logs: true,
            ..options()
        },
    )
    .expect("exports");
    assert!(
        result
            .included
            .iter()
            .any(|section| section == "debug-logs"),
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

    let all = super::super::diag_logs_tail(&state, None).expect("reads");
    assert_eq!(all.len(), 3, "every line, debug included: {all:?}");
    assert!(
        all.iter().any(|line| line.contains("third")),
        "the debug line is missing: {all:?}"
    );

    // The count is honoured, and the tail is the newest end — a user asking for the last line wants the last line, not
    // the first.
    let last = super::super::diag_logs_tail(&state, Some(1)).expect("reads");
    assert_eq!(last.len(), 1);
    assert!(last[0].contains("third"), "{last:?}");
}

#[test]
fn the_log_tail_is_empty_rather_than_an_error_when_nothing_has_been_written() {
    // A fresh installation has not logged anything yet. Reporting that as a failure would make a working install look
    // broken the first time a user opened diagnostics.
    let root = testing::Root::fresh("diag-log-tail-empty");
    let state = testing::state_in(&root);

    let lines = super::super::diag_logs_tail(&state, None).expect("an absent log is not a failure");
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
        &synra_ipc::DiagExportOptions {
            include_debug_logs: true,
            ..options()
        },
    )
    .expect("exports");

    assert!(
        result
            .included
            .iter()
            .any(|section| section == "debug-logs"),
        "a section the user asked for is listed even when it has nothing in it: {:?}",
        result.included
    );
}
