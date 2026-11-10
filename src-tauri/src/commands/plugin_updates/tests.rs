use super::*;

fn update(plugin: &str) -> UpdateInfo {
    UpdateInfo {
        plugin_id: synra_protocol::PluginId::parse(plugin).expect("a plugin id"),
        version: "2.0.0".to_owned(),
        needs_new_consent: false,
        added_capabilities: Vec::new(),
    }
}

#[test]
fn a_device_that_never_touched_the_setting_checks_for_updates() {
    let state = crate::testing::state();
    assert!(
        enabled(&state),
        "absent means on, which is the design's 24-hour default"
    );

    state
        .db()
        .set_config(
            AUTO_UPDATE_KEY,
            &serde_json::json!("never"),
            synra_store::sqlite::now_ms(),
        )
        .expect("the setting is written");
    assert!(!enabled(&state), "the switch says off");
}

#[test]
fn an_update_pass_sends_one_notice_naming_how_many() {
    // ★ One notice, not one per plugin: the next step is the same for all of them, and a stack of identical banners is
    // what a person learns to close without reading.
    let state = crate::testing::state();
    let subscriber = state.events.subscribe();
    let updates = [update("@acme/one"), update("@acme/two")];

    announce(&state, &updates);

    let delivery = state.events.take(subscriber).expect("one event");
    match delivery.events().first() {
        Some(KernelEvent::Notice { message, params }) => {
            assert_eq!(message, "notice.updates");
            assert_eq!(params["count"], 2);
            assert_eq!(params["plugins"][0], "@acme/one");
        }
        other => panic!("expected an updates notice, got {other:?}"),
    }
}

#[test]
fn a_check_is_due_once_a_day_and_after_never() {
    // ★ The bound is on the check, not on the sleep: the loop wakes every five minutes so a setting change takes
    // effect, and this is what keeps those wakes from becoming registry requests.
    assert!(due(None, None, 0), "a device that has never checked is due");
    assert!(
        !due(Some(1_000), None, 1_000 + CHECK_EVERY_MS - 1),
        "not yet"
    );
    assert!(
        due(Some(1_000), None, 1_000 + CHECK_EVERY_MS),
        "the day has passed"
    );
    assert!(
        due(Some(10_000), None, 1_000),
        "a clock that went backwards is due, not silent"
    );
}

#[test]
fn a_saved_check_publishes_its_card_and_a_negative_check_clears_it() {
    let state = crate::testing::state();
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
    let subscriber = state.events.subscribe();
    let mut check = PluginUpdateCheck {
        plugin_id: plugin.to_owned(),
        current_version: "1.0.0".to_owned(),
        checked_at_ms: 123,
        registry: None,
        available_version: Some("2.0.0".to_owned()),
        added_capabilities: vec!["log:*".to_owned()],
        attempts: 0,
        retry_at_ms: None,
    };
    assert!(record(&state, &check).expect("positive result").is_some());
    let delivery = state.events.take(subscriber).expect("card event");
    let Some(KernelEvent::PluginUpdated(card)) = delivery.events().first() else {
        panic!("expected a card event");
    };
    assert_eq!(
        card.update.as_ref().map(|info| info.version.as_str()),
        Some("2.0.0")
    );
    assert_eq!(card.update_checked_at_ms, Some(123));
    check.available_version = None;
    check.added_capabilities.clear();
    check.checked_at_ms = 456;
    assert!(record(&state, &check).expect("negative result").is_none());
    let delivery = state.events.take(subscriber).expect("card event");
    let Some(KernelEvent::PluginUpdated(card)) = delivery.events().first() else {
        panic!("expected a card event");
    };
    assert!(card.update.is_none());
    assert_eq!(card.update_checked_at_ms, Some(456));
}
