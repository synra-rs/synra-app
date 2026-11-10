use synra_ipc::ConfigPatch;

use super::{set, unset};
use crate::commands::config::{config_get, config_set};
use crate::testing;

#[test]
fn update_policies_are_explicit_and_invalid_values_do_not_write() {
    let state = testing::state();
    for value in [
        serde_json::json!(true),
        serde_json::json!("auto"),
        serde_json::Value::Null,
    ] {
        assert!(config_set(&state, &set(&[("plugins.auto_update", value)])).is_err());
    }
    assert!(state.db().config().expect("config").is_empty());
    for policy in ["never", "notify-only", "auto-minor", "auto-all"] {
        config_set(
            &state,
            &set(&[("plugins.auto_update", serde_json::json!(policy))]),
        )
        .expect("valid policy");
        assert_eq!(
            config_get(&state).expect("config").values["plugins.auto_update"],
            policy
        );
    }
}

#[test]
fn a_patch_that_sets_and_unsets_one_key_is_refused() {
    // ★ Refused, not resolved.
    //
    // Both halves were asked for and they contradict; choosing one would produce a setting that is the opposite of
    // what the caller wanted half the time, and the caller would not find out which.
    let state = testing::state();
    let patch = ConfigPatch {
        set: [("ui.locale".to_owned(), serde_json::json!("zh"))]
            .into_iter()
            .collect(),
        unset: vec!["ui.locale".to_owned()],
    };
    assert!(
        config_set(&state, &patch).is_err(),
        "the contradiction is not resolved for the caller"
    );
}

#[test]
fn a_refused_patch_writes_nothing_at_all() {
    // ★ The whole point of refusing.
    //
    // Refusing only the contradictory key while writing the rest would leave a half-applied patch, which is the state
    // that is hardest to reason about afterwards.
    let state = testing::state();
    let patch = ConfigPatch {
        set: [
            ("ui.locale".to_owned(), serde_json::json!("zh")),
            ("ui.theme".to_owned(), serde_json::json!("dark")),
        ]
        .into_iter()
        .collect(),
        unset: vec!["ui.locale".to_owned()],
    };
    assert!(config_set(&state, &patch).is_err());
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({}),
        "nothing was written"
    );
}

#[test]
fn a_patch_can_set_and_remove_at_the_same_time() {
    let state = testing::state();
    config_set(
        &state,
        &set(&[
            ("ui.locale", serde_json::json!("zh")),
            ("ui.theme", serde_json::json!("dark")),
        ]),
    )
    .expect("writes");
    let patch = ConfigPatch {
        set: [("sync.enabled".to_owned(), serde_json::json!(true))]
            .into_iter()
            .collect(),
        unset: vec!["ui.theme".to_owned()],
    };
    config_set(&state, &patch).expect("applies");
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({"ui.locale": "zh", "sync.enabled": true})
    );
}

#[test]
fn an_empty_patch_succeeds_and_writes_nothing() {
    // ★ Because a settings screen sends one the moment it opens.
    let state = testing::state();
    assert!(
        config_set(&state, &ConfigPatch::default()).is_ok(),
        "opening a screen is not an error"
    );
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({})
    );
}

#[test]
fn removing_a_key_that_was_never_there_still_succeeds() {
    // ★ Distinguished on purpose.
    //
    // "Reset to default" on a setting that is already the default is the common case, not a mistake, and answering it
    // with an error would make the affordance look broken for a screen that is doing the right thing.
    let state = testing::state();
    assert!(config_set(&state, &unset(&["ui.locale"])).is_ok());
}
