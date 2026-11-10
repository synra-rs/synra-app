use super::set;
use super::unset;
use crate::commands::config::config_set;
use crate::testing;

#[test]
fn changing_registries_announces_cards_without_the_previous_sources_updates() {
    let state = testing::state();
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
        .save_plugin_update_check(&synra_store::PluginUpdateCheck {
            plugin_id: plugin.to_owned(),
            current_version: "1.0.0".to_owned(),
            checked_at_ms: 123,
            registry: None,
            available_version: Some("2.0.0".to_owned()),
            added_capabilities: Vec::new(),
            attempts: 0,
            retry_at_ms: None,
        })
        .expect("check");
    let subscriber = state.events.subscribe();
    config_set(
        &state,
        &set(&[(
            "plugins.registry",
            serde_json::json!("https://registry.example"),
        )]),
    )
    .expect("switch");
    let delivery = state.events.take(subscriber).expect("events");
    let card = delivery
        .events()
        .iter()
        .find_map(|event| match event {
            synra_ipc::event::KernelEvent::PluginUpdated(card) => Some(card),
            _ => None,
        })
        .expect("card updated");
    assert!(card.update.is_none());
    assert!(card.update_checked_at_ms.is_none());
}

#[test]
fn a_written_setting_is_announced_so_another_window_agrees() {
    // ★ Two windows on one kernel is the case this is for: the second one has no way to learn that the first changed a
    // setting except this event. The snapshot carries the whole configuration, so the reader does not have to apply
    // the patch itself — the patch is the writer's input, not a description of the result.
    let state = testing::state();
    let subscriber = state.events.subscribe();

    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");

    let delivery = state.events.take(subscriber).expect("announced");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::ConfigUpdated(snapshot)] => {
            assert_eq!(snapshot.values, serde_json::json!({"ui.locale": "zh"}));
        }
        other => panic!("expected the configuration, got {other:?}"),
    }
}

#[test]
fn removing_a_setting_is_announced_without_it() {
    // ★ The **other** path through `config_set`: a patch that removes something skips the early return, verifies the
    // removal against the table, and announces on the way out. The bug this covers is real — a publish placed only at
    // the end of the common path was silent here, and the removal would reach one window only.
    let state = testing::state();
    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
    let subscriber = state.events.subscribe();

    config_set(&state, &unset(&["ui.locale"])).expect("removes");

    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::ConfigUpdated(snapshot)] => {
            assert_eq!(
                snapshot.values,
                serde_json::json!({}),
                "the reader is told what is left, not what was removed"
            );
            assert!(snapshot.overridden.is_empty());
        }
        other => panic!("expected the configuration, got {other:?}"),
    }
}

#[test]
fn a_refused_patch_announces_nothing() {
    // A reader told "the configuration changed" when it did not would re-read, find the old values, and conclude the
    // write was lost. The announcement is after the checks for that reason.
    let state = testing::state();
    let subscriber = state.events.subscribe();
    let conflicting = set(&[("ui.locale", serde_json::json!("zh"))]);
    let mut patch = conflicting;
    patch.unset.push("ui.locale".to_owned());

    assert!(config_set(&state, &patch).is_err(), "it is refused");
    assert!(
        state.events.take(subscriber).is_none(),
        "and nothing is said"
    );
}
