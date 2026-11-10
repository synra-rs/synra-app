use super::*;

#[test]
fn the_daily_pass_applies_only_the_updates_the_user_and_verified_package_allow() {
    for (version, selected, seed, added, applies) in [
        ("1.0.1", "auto-minor", 7, 0, true),
        ("2.0.0", "auto-all", 7, 0, true),
        ("1.1.0", "auto-minor", 7, 2, true),
        ("2.0.0", "auto-minor", 7, 0, false),
        ("1.1.0", "auto-all", 9, 0, false),
        ("1.1.0", "auto-all", 7, 1, false),
        ("1.1.0", "notify-only", 7, 0, false),
        ("1.1.0", "never", 7, 0, false),
    ] {
        let registry = Registry::start();
        let (root, state) = installed_state();
        let next = root.path().join("next");
        signed_source(&next, version, seed, added);
        publish(&registry, &next, version);
        state
            .db()
            .set_config(
                crate::commands::registry::REGISTRY_KEY,
                &serde_json::json!(registry.base),
                1,
            )
            .expect("registry");
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!(selected), 1)
            .expect("policy");
        let subscriber = state.events.subscribe();
        let updates = check_all(&state, 1, &Cancellation::new());
        let expected = if applies { version } else { "1.0.0" };
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("row")
                .expect("installed")
                .version,
            expected,
            "{selected}: {version}, seed {seed}, added {added}"
        );
        let id = PluginId::parse("@acme/hello").expect("id");
        assert_eq!(
            state.plugin_set().expect("engine").lock().version(&id),
            Some(expected),
            "running version agrees with disk"
        );
        assert_eq!(updates.is_empty(), applies || selected == "never");
        let events = state.events.take(subscriber);
        assert_eq!(events.as_ref().is_some_and(|events| events.events().iter().any(|event| matches!(event, KernelEvent::Notice { message, .. } if message == "notice.updated"))), applies);
        if selected == "notify-only" {
            assert_eq!(registry.requests.load(Ordering::SeqCst), 1);
        }
        if selected == "never" {
            assert_eq!(registry.requests.load(Ordering::SeqCst), 0);
        }
        if applies && added == 2 {
            signed_source(&next, "1.2.0", seed, 2);
            publish(&registry, &next, "1.2.0");
            assert!(
                check_all(
                    &state,
                    synra_store::sqlite::now_ms() + CHECK_EVERY_MS,
                    &Cancellation::new()
                )
                .is_empty()
            );
            assert_eq!(
                state.plugin_set().expect("engine").lock().version(&id),
                Some("1.2.0"),
                "consecutive permissionless updates keep running"
            );
        }
        let cache = state.paths.cache_dir().join("registry");
        assert!(
            !cache.exists() || cache.read_dir().expect("cache").next().is_none(),
            "download cleaned up"
        );
    }
}
