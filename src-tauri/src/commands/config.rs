//! The configuration, as the settings screens read and write it.
//!
//! Two commands, and the pairing of them is the point: a screen that can read settings it cannot write is a screen that
//! looks broken, and one that can write without reading back is a screen that silently disagrees with the next start.

use synra_ipc::{ConfigPatch, ConfigSnapshot, IpcError};
use synra_protocol::ErrorCode;
use synra_store::ConfigChange;

use crate::state::AppState;

/// Reads the configuration.
///
/// # Errors
///
/// * [`IpcError::Internal`] when the rows cannot be read.
/// * [`IpcError::InvalidArgument`] when a stored value is not JSON text.
///
/// # Why a malformed value is a refusal rather than being skipped
///
/// Because a settings screen that shows eight of nine settings, with no indication that the ninth is broken, is a screen
/// whose user will turn the ninth one on and find it does not take. Naming the key is the only thing that makes it
/// diagnosable, and the key is the one thing that survives the value being unreadable.
pub fn config_get(state: &AppState) -> Result<ConfigSnapshot, IpcError> {
    let rows = {
        let db = state.db();
        db.config().map_err(|error| internal(&error))?
    };

    let mut values = serde_json::Map::new();
    let mut overridden = Vec::new();
    for row in &rows {
        values.insert(row.key.clone(), row_value(row)?);
        // ★ Every stored key is an override.
        //
        // A row exists because a file or a screen put one there, so the list is the row keys rather than a separate
        // record of provenance that could disagree with them.
        overridden.push(row.key.clone());
    }

    Ok(ConfigSnapshot { values: serde_json::Value::Object(values), overridden })
}

/// Turns one row's stored text into a value, naming the key if it cannot.
///
/// # Why this is a function and not inline
///
/// Because the row it has to handle — text in the column that does not parse — cannot be produced through any API this
/// crate has. The writer always serialises. So a test can only reach this by calling it with a row it built by hand, and
/// a rule that cannot be reached is a rule that was never checked.
fn row_value(row: &synra_store::ConfigRow) -> Result<serde_json::Value, IpcError> {
    serde_json::from_str(&row.value).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "key": row.key,
            "reason": format!("the stored value is not readable: {error}"),
        }))
    })
}

/// Writes a patch.
///
/// # Errors
///
/// * [`IpcError::InvalidArgument`] when the patch sets and unsets the same key, or is empty of keys to write.
///
/// The conflict is refused rather than resolved by precedence: a caller that asked for both meant one of them, and
/// picking for it produces a setting that is the opposite of what it wanted half the time. The kernel's own
/// [`ConfigPatch::has_conflict`] is what decides, so there is one rule rather than one per host.
///
/// An empty patch is *accepted* rather than refused — a settings screen sends one when it opens, before the user has
/// touched anything, and answering that with an error makes the screen look broken on load.
pub fn config_set(state: &AppState, patch: &ConfigPatch) -> Result<(), IpcError> {
    if let Some(key) = patch.has_conflict() {
        return Err(
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"key": key, "reason": "a key cannot be both set and unset"})),
        );
    }

    if let Some(value) = patch.set.get(super::plugin_updates::AUTO_UPDATE_KEY)
        && !matches!(value.as_str(), Some("never" | "notify-only" | "auto-minor" | "auto-all"))
    {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "key": super::plugin_updates::AUTO_UPDATE_KEY,
            "reason": "select never, notify-only, auto-minor or auto-all",
        })));
    }

    let changes: Vec<ConfigChange> = patch
        .set
        .iter()
        .map(|(key, value)| ConfigChange { key: key.clone(), value: Some(value.clone()) })
        .chain(patch.unset.iter().map(|key| ConfigChange { key: key.clone(), value: None }))
        .collect();

    {
        let db = state.db();
        db.apply_config(&changes, state.started_at_ms).map_err(|error| internal(&error))?;
    }

    // Cached updates belong to the registry that answered them. Re-publish the cards after changing sources so an
    // already open plugins page drops the old answers as soon as the configuration write succeeds.
    if changes.iter().any(|change| change.key == super::registry::REGISTRY_KEY) {
        let rows = state.db().plugins().map_err(|error| internal(&error))?;
        for row in rows {
            let id = synra_protocol::PluginId::parse(&row.plugin_id).map_err(|error| {
                IpcError::unknown(ErrorCode::Internal)
                    .with_params(serde_json::json!({"detail": error.to_string()}))
            })?;
            super::plugins::announce(state, &id);
        }
    }

    // ★ Read back, and compared.
    //
    // A write that matched no row reports success — SQLite does not tell a `DELETE` that found nothing from one that
    // found a row. Re-reading is what turns "the statement ran" into "the setting is there", and it is cheap enough here
    // to be worth the certainty. Only removals are checked, because a value the store refused to serialise is already an
    // error rather than a silent success.
    if patch.unset.is_empty() {
        // ★ Announced on **both** paths, and this is the common one: a patch that only sets values returns here, so a
        // publish at the end of the function would never run for the case the interface uses most.
        announce_config(state);
        return Ok(());
    }
    let remaining = {
        let db = state.db();
        db.config().map_err(|error| internal(&error))?
    };
    let still_there: Vec<&String> = remaining
        .iter()
        .filter(|row| patch.unset.iter().any(|key| key == &row.key))
        .map(|row| &row.key)
        .collect();
    if !still_there.is_empty() {
        return Err(
            IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"keys": still_there, "reason": "the keys were not removed"})),
        );
    }
    // ★ Announced here as well as in the early-return above: this path is a patch that also removed something, and the
    // order of the two calls is what keeps "refused" from being announced — every refusal above returned before either.
    announce_config(state);
    Ok(())
}

/// Announces the configuration as it now stands.
///
/// Silent when the read-back fails: an event is not where an error goes, and the next `config_get` is the truth either way.
fn announce_config(state: &AppState) {
    if let Ok(snapshot) = config_get(state) {
        state.publish(synra_ipc::event::KernelEvent::ConfigUpdated(Box::new(snapshot)));
    }
}

/// The one place a database failure becomes an `IpcError`, so every command here reports it identically.
///
/// By reference because the message is taken and the error is then dropped: taking it by value would move a
/// [`DbError`] nobody can use again, for a message that is built and immediately consumed.
fn internal(error: &synra_store::DbError) -> IpcError {
    IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({ "reason": error.to_string() }))
}

#[cfg(test)]
mod tests {
    use synra_ipc::ConfigPatch;

    use super::{config_get, config_set};
    use crate::testing;

    #[test]
    fn update_policies_are_explicit_and_invalid_values_do_not_write() {
        let state = testing::state();
        for value in [serde_json::json!(true), serde_json::json!("auto"), serde_json::Value::Null] {
            assert!(config_set(&state, &set(&[("plugins.auto_update", value)])).is_err());
        }
        assert!(state.db().config().expect("config").is_empty());
        for policy in ["never", "notify-only", "auto-minor", "auto-all"] {
            config_set(&state, &set(&[("plugins.auto_update", serde_json::json!(policy))])).expect("valid policy");
            assert_eq!(config_get(&state).expect("config").values["plugins.auto_update"], policy);
        }
    }

    fn set(kv: &[(&str, serde_json::Value)]) -> ConfigPatch {
        ConfigPatch {
            set: kv.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect(),
            unset: Vec::new(),
        }
    }

    fn unset(keys: &[&str]) -> ConfigPatch {
        ConfigPatch { set: Default::default(), unset: keys.iter().map(|k| (*k).to_owned()).collect() }
    }

    #[test]
    fn an_untouched_configuration_reads_as_an_empty_one() {
        let state = testing::state();
        let snapshot = config_get(&state).expect("a snapshot");
        assert_eq!(snapshot.values, serde_json::json!({}), "there are no defaults yet");
        assert!(snapshot.overridden.is_empty());
    }

    #[test]
    fn a_written_setting_reads_back_with_its_value() {
        let state = testing::state();
        config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({"ui.locale": "zh"}));
    }

    #[test]
    fn changing_registries_announces_cards_without_the_previous_sources_updates() {
        let state = testing::state();
        let plugin = "@acme/hello";
        state.db().insert_plugin(plugin, false, false, "good", 1, 1).expect("plugin");
        state.db().record_plugin_version(&synra_store::PluginVersionRecord { plugin_id: plugin, version: "1.0.0", artifact: "wasm", digest: "digest", manifest: "{}", installed_ms: 1, ..Default::default() }).expect("version");
        state.db().save_plugin_update_check(&synra_store::PluginUpdateCheck {
            plugin_id: plugin.to_owned(),
            current_version: "1.0.0".to_owned(),
            checked_at_ms: 123,
            registry: None,
            available_version: Some("2.0.0".to_owned()),
            added_capabilities: Vec::new(),
            attempts: 0,
            retry_at_ms: None,
        }).expect("check");
        let subscriber = state.events.subscribe();
        config_set(&state, &set(&[("plugins.registry", serde_json::json!("https://registry.example"))])).expect("switch");
        let delivery = state.events.take(subscriber).expect("events");
        let card = delivery.events().iter().find_map(|event| match event {
            synra_ipc::event::KernelEvent::PluginUpdated(card) => Some(card),
            _ => None,
        }).expect("card updated");
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
        assert!(state.events.take(subscriber).is_none(), "and nothing is said");
    }

    #[test]
    fn a_written_setting_is_listed_as_overridden() {
        // ★ Because the screen uses this to draw a "reset" affordance, and a setting the user changed but cannot get
        // back from the default is the bug this column exists to prevent.
        let state = testing::state();
        config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
        assert_eq!(config_get(&state).expect("a snapshot").overridden, vec!["ui.locale".to_owned()]);
    }

    #[test]
    fn a_value_of_any_json_shape_comes_back_as_it_went_in() {
        let state = testing::state();
        let cases = [
            ("a.number", serde_json::json!(7)),
            ("a.boolean", serde_json::json!(false)),
            ("a.list", serde_json::json!(["x"])),
            ("an.object", serde_json::json!({"k": true})),
        ];
        config_set(&state, &set(&cases)).expect("writes");
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({
            "a.number": 7, "a.boolean": false, "a.list": ["x"], "an.object": {"k": true}
        }));
    }

    #[test]
    fn a_second_write_to_the_same_key_replaces_it_rather_than_adding_one() {
        let state = testing::state();
        config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("first");
        config_set(&state, &set(&[("ui.locale", serde_json::json!("en"))])).expect("second");
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({"ui.locale": "en"}));
    }

    #[test]
    fn removing_a_setting_takes_it_back_to_absent() {
        let state = testing::state();
        config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
        config_set(&state, &unset(&["ui.locale"])).expect("removes");
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({}));
    }

    #[test]
    fn a_patch_that_sets_and_unsets_one_key_is_refused() {
        // ★ Refused, not resolved.
        //
        // Both halves were asked for and they contradict; choosing one would produce a setting that is the opposite of
        // what the caller wanted half the time, and the caller would not find out which.
        let state = testing::state();
        let patch = ConfigPatch {
            set: [("ui.locale".to_owned(), serde_json::json!("zh"))].into_iter().collect(),
            unset: vec!["ui.locale".to_owned()],
        };
        assert!(config_set(&state, &patch).is_err(), "the contradiction is not resolved for the caller");
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
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({}), "nothing was written");
    }

    #[test]
    fn a_patch_can_set_and_remove_at_the_same_time() {
        let state = testing::state();
        config_set(&state, &set(&[("ui.locale", serde_json::json!("zh")), ("ui.theme", serde_json::json!("dark"))]))
            .expect("writes");
        let patch = ConfigPatch {
            set: [("sync.enabled".to_owned(), serde_json::json!(true))].into_iter().collect(),
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
        assert!(config_set(&state, &ConfigPatch::default()).is_ok(), "opening a screen is not an error");
        assert_eq!(config_get(&state).expect("a snapshot").values, serde_json::json!({}));
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

    #[test]
    fn a_setting_written_by_one_state_is_read_by_the_next_one() {
        // ★ Durability, across a restart rather than across a re-read.
        //
        // Every other test here reads through the same `AppState`, so none of them would notice a setting that lives in
        // memory and is not written down — which is exactly the state this command exists to prevent.
        let root = testing::Root::fresh("config");
        {
            let first = testing::state_in(&root);
            config_set(&first, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
        }
        let second = testing::state_in(&root);
        assert_eq!(
            config_get(&second).expect("a snapshot").values,
            serde_json::json!({"ui.locale": "zh"}),
            "a setting that did not survive the process was never saved"
        );
    }

    #[test]
    fn a_stored_value_that_is_not_json_is_reported_with_its_key() {
        // ★ The row is built by hand, and for a stated reason.
        //
        // No code path produces it — the only writer serialises — so a test that could not make it would never check the
        // thing that matters: that a corrupt row names its key. A screen that silently drops one of nine settings leaves
        // the user with no way to find the ninth.
        let row = synra_store::ConfigRow {
            key: "ui.locale".to_owned(),
            value: "not json".to_owned(),
            updated_ms: 1,
        };

        let error = super::row_value(&row).expect_err("a corrupt row is refused");
        assert_eq!(error.code, synra_protocol::ErrorCode::InvalidArgument);
        assert_eq!(
            error.params.get("key").and_then(|value| value.as_str()),
            Some("ui.locale"),
            "and it says which setting is broken"
        );
    }

    #[test]
    fn every_json_shape_the_writer_can_produce_is_read_back_as_it_went_in() {
        // ★ Across the raw-text boundary rather than through a database.
        //
        // The store's own tests cover the round trip; this one covers the second parse, in this crate, of text that has
        // made a round trip. A reader that is checked only against a `serde_json::Value` it just built never checks what
        // the text actually is.
        for value in [
            serde_json::json!("zh"),
            serde_json::json!(7),
            serde_json::json!(false),
            serde_json::json!(["x"]),
            serde_json::json!({"k": "v"}),
            serde_json::Value::Null,
        ] {
            let row = synra_store::ConfigRow {
                key: "k".to_owned(),
                value: serde_json::to_string(&value).expect("serialises"),
                updated_ms: 1,
            };
            assert_eq!(super::row_value(&row).expect("parses"), value);
        }
    }

    #[test]
    fn one_bad_row_does_not_stop_another_from_being_read() {
        // ★ Why the parse is per row rather than once over a whole map of text.
        //
        // The interface asks for every setting at once. Parsing them together and failing on the first bad one would mean
        // a single corrupt row hides every setting after it in key order — so a user with one bad key and nine good ones
        // would see a different number of settings depending on how their keys happen to be spelled.
        let bad = synra_store::ConfigRow {
            key: "ui.locale".to_owned(),
            value: "not json".to_owned(),
            updated_ms: 1,
        };
        let good = synra_store::ConfigRow {
            key: "ui.theme".to_owned(),
            value: "\"dark\"".to_owned(),
            updated_ms: 1,
        };

        assert_eq!(super::row_value(&good).expect("the good row still reads"), serde_json::json!("dark"));
        let error = super::row_value(&bad).expect_err("the bad row is still refused");
        assert_eq!(error.params.get("key").and_then(|value| value.as_str()), Some("ui.locale"));
    }
}
