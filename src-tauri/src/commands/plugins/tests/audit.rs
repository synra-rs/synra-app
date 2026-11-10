use super::*;

#[test]
fn a_recorded_call_is_mapped_from_its_row() {
    // ★ What this command is responsible for. The table's own read is tested where it lives; what is left here is the
    // mapping, and a wrong mapping looks right — an allowed call reported with a reason, or a refusal reported as
    // allowed.
    let allowed = record_card(&row(ID, "log:info", "allowed", 1_700_000_000_000));
    assert_eq!(allowed.capability, "log:info");
    assert!(allowed.allowed);
    assert_eq!(allowed.elapsed_ms, Some(7));
    assert_eq!(
        allowed.reason, None,
        "an allowed call has no reason to give"
    );

    let denied = record_card(&row(ID, "device:list", "denied", 1_700_000_000_001));
    assert!(!denied.allowed);
    // The outcome's stable name, which the interface translates — not a sentence written here.
    assert_eq!(denied.reason.as_deref(), Some("denied"));
    // A row recorded without an action behind it stays without one — `None` here is a fact about the call, not a
    // mapping that failed.
    assert_eq!(denied.trace_id, None);

    // ★ And a row with one gives the DTO one. The column is new; this is the half that proves the mapping reads it
    // rather than still answering `None` from an older shape.
    let mut traced = row(ID, "device:list", "allowed", 1_700_000_000_003);
    traced.trace_id = Some("01a150c8-c332-768d-9dd1-981502aa1854".to_owned());
    assert_eq!(
        record_card(&traced).trace_id.as_deref(),
        Some("01a150c8-c332-768d-9dd1-981502aa1854")
    );

    // ★ "Permitted by the gate, failed in the operation" is not a refusal — `AuditOutcome::was_allowed` says so, and a
    // mapping that compared the name against `"allowed"` would draw this call as blocked.
    let failed = record_card(&row(ID, "network:fetch", "failed", 1_700_000_000_002));
    assert!(failed.allowed, "the gate permitted this call");
    assert_eq!(failed.reason, None);

    // A name from a newer build is reported rather than guessed at: presenting it as allowed could hide a refusal.
    let unknown = record_card(&row(
        ID,
        "device:list",
        "from-a-newer-build",
        1_700_000_000_003,
    ));
    assert!(!unknown.allowed);
    assert_eq!(unknown.reason.as_deref(), Some("from-a-newer-build"));
}

#[test]
fn the_audit_reads_the_trail_that_outlives_the_engine() {
    // ★ The fixture has no engine, and that must not matter: the table is written by whichever host ran the call, so a
    // build that cannot run plugins can still answer what this database recorded. The old `Unsupported` answer was
    // true only while the in-memory ring was the whole trail.
    let state = crate::testing::state();
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", 5_000))
        .expect("recorded");

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].capability, "device:list");
    assert_eq!(rows[0].at_ms, 5_000);
}

#[test]
fn the_audit_read_can_skip_the_rows_a_page_already_showed() {
    // ★ "Show more" is wired through this parameter. A command that ignored it would repeat the first page, which is
    // the one failure a list that appends rather than replaces would show as duplicated rows rather than as nothing.
    let state = crate::testing::state();
    for at in [1_000, 2_000, 3_000] {
        state
            .db()
            .record_audit(&row(ID, "device:list", "allowed", at))
            .expect("recorded");
    }

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 1, 1).expect("read");

    assert_eq!(rows.len(), 1, "one row asked for, one row answered");
    assert_eq!(
        rows[0].at_ms, 2_000,
        "and it is the second-newest, not the newest again"
    );
}

#[test]
fn clearing_forgets_the_durable_rows_and_leaves_another_plugin() {
    // The same rule as the ring, now on the copy that survives a restart.
    let state = crate::testing::state();
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", 5_000))
        .expect("recorded");
    state
        .db()
        .record_audit(&row("@acme/other", "log:info", "allowed", 6_000))
        .expect("recorded");

    plugins_audit_clear(&state, &PluginId::parse(ID).expect("a plugin id")).expect("cleared");

    assert_eq!(state.db().audit_count().expect("counted"), 1);
    assert!(
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0)
            .expect("read")
            .is_empty()
    );
}

#[test]
fn old_calls_are_pruned_and_recent_ones_kept() {
    // ★ The table is written on every gated call and nothing in this shell swept it, so a long-running install would
    // grow it for ever. With no configured window the design's seven days is the rule.
    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 10 * day))
        .expect("recorded");
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 2 * day))
        .expect("recorded");

    assert_eq!(prune_audit(&state, now), 1);

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at_ms, now - 2 * day, "the recent call stayed");
}

#[test]
fn the_retention_window_comes_from_the_configuration() {
    // ★ The setting existed only as a constant. Reading it is what turns "the design names a key" into the key doing
    // something: a one-day window has to remove the two-day-old row that the default seven kept in the test above.
    use synra_store::ConfigChange;

    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 10 * day))
        .expect("recorded");
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 2 * day))
        .expect("recorded");
    state
        .db()
        .apply_config(
            &[ConfigChange {
                key: AUDIT_RETENTION_KEY.to_owned(),
                value: Some(serde_json::json!(1)),
            }],
            now,
        )
        .expect("the setting is written");

    assert_eq!(
        prune_audit(&state, now),
        2,
        "a one-day window removes both calls"
    );
}

#[test]
fn a_retention_outside_the_sane_range_is_clamped_rather_than_trusted() {
    // ★ The file can be edited by hand, and `0` would delete today's calls — including the refusal the user just
    // looked at. The clamp is what makes the setting a window rather than a way to empty the table from a typo.
    use synra_store::ConfigChange;

    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now))
        .expect("recorded");
    state
        .db()
        .apply_config(
            &[ConfigChange {
                key: AUDIT_RETENTION_KEY.to_owned(),
                value: Some(serde_json::json!(0)),
            }],
            now,
        )
        .expect("the setting is written");

    assert_eq!(
        prune_audit(&state, now),
        0,
        "zero becomes the one-day minimum, and today's call stays"
    );
}
