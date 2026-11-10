use super::*;

#[test]
fn failed_downloads_back_off_across_restart_and_stop_fast_retries_after_three_attempts() {
    let registry = Registry::start();
    let (root, state) = installed_state();
    let next = root.path().join("retry-candidate");
    signed_source(&next, "1.1.0", 7, 0);
    publish(&registry, &next, "1.1.0");
    *registry.tarball.lock() = b"corrupt download".to_vec();
    state
        .db()
        .set_config(
            crate::commands::registry::REGISTRY_KEY,
            &serde_json::json!(registry.base),
            1,
        )
        .expect("source");
    state
        .db()
        .set_config(AUTO_UPDATE_KEY, &serde_json::json!("auto-all"), 1)
        .expect("policy");
    let cancel = Cancellation::new();
    assert_eq!(check_all(&state, 100, &cancel).len(), 1);
    let saved = state
        .db()
        .plugin_update_check("@acme/hello")
        .expect("read")
        .expect("check");
    assert_eq!(saved.attempts, 1);
    let restarted = crate::testing::reopen(&state);
    let mut next_attempt = saved.retry_at_ms.expect("retry deadline");
    for attempt in 2..=3 {
        let requests = registry.requests.load(Ordering::SeqCst);
        assert!(check_all(&restarted, next_attempt - 1, &cancel).is_empty());
        assert_eq!(
            registry.requests.load(Ordering::SeqCst),
            requests,
            "no network traffic before the deadline"
        );
        assert_eq!(check_all(&restarted, next_attempt, &cancel).len(), 1);
        assert!(
            registry.requests.load(Ordering::SeqCst) > requests,
            "a due retry contacts the real registry"
        );
        let saved = restarted
            .db()
            .plugin_update_check("@acme/hello")
            .expect("read")
            .expect("check");
        assert_eq!(saved.attempts, attempt);
        next_attempt = saved
            .retry_at_ms
            .unwrap_or(saved.checked_at_ms + CHECK_EVERY_MS);
        assert_eq!(saved.retry_at_ms.is_none(), attempt == 3);
    }
    let requests = registry.requests.load(Ordering::SeqCst);
    assert!(check_all(&restarted, next_attempt - 1, &cancel).is_empty());
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        requests,
        "the exhausted fast path waits for the day"
    );
    assert_eq!(
        restarted
            .db()
            .active_plugin("@acme/hello")
            .expect("read")
            .expect("installed")
            .version,
        "1.0.0"
    );
}

#[test]
fn a_restart_keeps_daily_checks_and_an_offline_attempt_keeps_the_last_answer() {
    let registry = Registry::start();
    let (_root, state) = crate::testing::file_state();
    let plugin = "@acme/hello";
    state
        .db()
        .insert_plugin(plugin, false, false, "good", 1, 1)
        .expect("plugin");
    state
        .db()
        .record_plugin_version(&synra_store::PluginVersionRecord {
            plugin_id: plugin,
            version: "1.0.0",
            artifact: "wasm",
            digest: "digest",
            manifest: "{}",
            installed_ms: 1,
            ..Default::default()
        })
        .expect("version");
    state
        .db()
        .set_config(
            crate::commands::registry::REGISTRY_KEY,
            &serde_json::json!(registry.base),
            1,
        )
        .expect("registry");
    let cancel = Cancellation::new();
    let updates = check_all(&state, 1, &cancel);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].version, "2.0.0");
    let saved = state
        .db()
        .plugin_update_check(plugin)
        .expect("check")
        .expect("saved");
    let restarted = crate::testing::reopen(&state);
    assert!(
        check_all(
            &restarted,
            saved.checked_at_ms + CHECK_EVERY_MS - 1,
            &cancel
        )
        .is_empty()
    );
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        1,
        "restart does not recheck early"
    );
    assert_eq!(
        crate::commands::plugins::plugins_updates(&restarted).expect("updates"),
        updates
    );

    *registry.body.lock() = "unreadable registry response".to_owned();
    assert!(check_all(&restarted, saved.checked_at_ms + CHECK_EVERY_MS, &cancel).is_empty());
    assert_eq!(registry.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        restarted.db().plugin_update_check(plugin).expect("check"),
        Some(saved.clone())
    );

    *registry.body.lock() = document("1.0.0");
    assert!(check_all(&restarted, saved.checked_at_ms + CHECK_EVERY_MS, &cancel).is_empty());
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        3,
        "failure is retried at the next pass"
    );
    assert!(
        crate::commands::plugins::plugins_updates(&restarted)
            .expect("updates")
            .is_empty()
    );
    let negative = restarted
        .db()
        .plugin_update_check(plugin)
        .expect("check")
        .expect("negative check");
    assert!(negative.available_version.is_none());
    let restarted_again = crate::testing::reopen(&restarted);
    assert!(check_all(&restarted_again, negative.checked_at_ms + 1, &cancel).is_empty());
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        3,
        "negative checks also survive a restart"
    );
}
