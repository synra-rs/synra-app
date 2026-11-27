use super::*;

fn saved_check(state: &AppState, version: &str) -> PluginUpdateCheck {
    let db = state.db();
    let check = PluginUpdateCheck {
        plugin_id: "@acme/hello".to_owned(),
        current_version: "1.0.0".to_owned(),
        checked_at_ms: 1,
        request_order: db.begin_plugin_update_check().expect("reserve request"),
        registry: synra_core::plugin_updates::registry_setting(&db).expect("registry"),
        available_version: Some(version.to_owned()),
        added_capabilities: Vec::new(),
        attempts: 0,
        retry_at_ms: None,
    };
    assert!(db.save_plugin_update_check(&check).expect("save check"));
    check
}

#[test]
fn revoking_a_publisher_stops_and_untrusts_its_installed_plugin() {
    // ★ The decision used to change only the row the installer consults, so the code already running stayed untouched
    // until the next start. Revoke is about what is here now.
    let (_root, state) = installed_state();
    let fingerprint = state.db().publishers().expect("publishers")[0]
        .fingerprint
        .clone();
    let active = state
        .db()
        .active_plugin("@acme/hello")
        .expect("row")
        .expect("installed");
    assert_eq!(
        active.publisher_id.as_deref(),
        Some("@acme"),
        "the version records its signer"
    );
    assert_eq!(active.fingerprint.as_deref(), Some(fingerprint.as_str()));
    let id = synra_protocol::ids::PluginId::parse("@acme/hello").expect("a plugin id");
    assert!(
        state.plugin_set().expect("a set").lock().contains(&id),
        "the fixture starts with the plugin loaded"
    );

    crate::commands::plugins::plugins_publisher_decide(&state, "@acme", &fingerprint, "revoked")
        .expect("revoke the key");

    let row = state
        .db()
        .plugins()
        .expect("plugins")
        .into_iter()
        .find(|row| row.plugin_id == "@acme/hello")
        .expect("the plugin row");
    assert!(!row.enabled, "a revoked publisher's plugin is switched off");
    assert!(!row.trusted, "and its trust goes with the key");
    assert!(
        !state.plugin_set().expect("a set").lock().contains(&id),
        "the running instance is unloaded, not merely marked for the next start"
    );
}

#[test]
fn cancellation_policy_changes_and_mismatching_targets_leave_the_running_version_alone() {
    let (root, state) = installed_state();
    let next = root.path().join("next");
    signed_source(&next, "1.1.0", 7, 0);
    for (selected, cancelled, id, version) in [
        ("auto-all", true, "@acme/hello", "1.1.0"),
        ("never", false, "@acme/hello", "1.1.0"),
        ("auto-all", false, "@acme/other", "1.1.0"),
        ("auto-all", false, "@acme/hello", "2.0.0"),
    ] {
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!(selected), 1)
            .expect("policy");
        let cancel = Cancellation::new();
        if cancelled {
            cancel.cancel();
        }
        let mut check = saved_check(&state, version);
        check.plugin_id = id.to_owned();
        assert!(
            super::apply_automatic_update(
                &state,
                &check,
                &next,
                UpdatePolicy::AutoAll,
                &cancel
            )
            .is_err()
        );
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("version")
                .expect("installed")
                .version,
            "1.0.0"
        );
        assert_eq!(
            state
                .plugin_set()
                .expect("engine")
                .lock()
                .version(&PluginId::parse("@acme/hello").expect("id")),
            Some("1.0.0")
        );
    }
}

#[test]
fn a_changed_registry_or_revoked_candidate_leaves_the_running_version_untouched() {
    for revoked in [false, true] {
        let (root, state) = installed_state();
        let next = root.path().join("next");
        signed_source(&next, "1.1.0", 7, 0);
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!("auto-all"), 1)
            .expect("policy");
        state
            .db()
            .set_config(
                crate::commands::registry::REGISTRY_KEY,
                &serde_json::json!("https://original.example"),
                1,
            )
            .expect("source");
        let check = saved_check(&state, "1.1.0");
        if revoked {
            std::fs::write(state.paths.data_dir().join("revocations.json"), r#"{"revoked":[{"plugin":"@acme/hello","versions":["1.1.0"],"reason":"withdrawn"}]}"#).expect("revocations");
        } else {
            state
                .db()
                .set_config(
                    crate::commands::registry::REGISTRY_KEY,
                    &serde_json::json!("https://changed.example"),
                    1,
                )
                .expect("changed source");
        }
        assert!(
            super::apply_automatic_update(
                &state,
                &check,
                &next,
                UpdatePolicy::AutoAll,
                &Cancellation::new()
            )
            .is_err()
        );
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("row")
                .expect("installed")
                .version,
            "1.0.0"
        );
        assert_eq!(
            state
                .plugin_set()
                .expect("engine")
                .lock()
                .version(&PluginId::parse("@acme/hello").expect("id")),
            Some("1.0.0")
        );
    }
}
