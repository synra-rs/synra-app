use super::{set, unset};
use crate::commands::config::{config_get, config_set, row_value};
use crate::testing;

#[test]
fn an_untouched_configuration_reads_as_an_empty_one() {
    let state = testing::state();
    let snapshot = config_get(&state).expect("a snapshot");
    assert_eq!(
        snapshot.values,
        serde_json::json!({}),
        "there are no defaults yet"
    );
    assert!(snapshot.overridden.is_empty());
}

#[test]
fn a_written_setting_reads_back_with_its_value() {
    let state = testing::state();
    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({"ui.locale": "zh"})
    );
}

#[test]
fn a_written_setting_is_listed_as_overridden() {
    // ★ Because the screen uses this to draw a "reset" affordance, and a setting the user changed but cannot get
    // back from the default is the bug this column exists to prevent.
    let state = testing::state();
    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
    assert_eq!(
        config_get(&state).expect("a snapshot").overridden,
        vec!["ui.locale".to_owned()]
    );
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
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({
            "a.number": 7, "a.boolean": false, "a.list": ["x"], "an.object": {"k": true}
        })
    );
}

#[test]
fn a_second_write_to_the_same_key_replaces_it_rather_than_adding_one() {
    let state = testing::state();
    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("first");
    config_set(&state, &set(&[("ui.locale", serde_json::json!("en"))])).expect("second");
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({"ui.locale": "en"})
    );
}

#[test]
fn removing_a_setting_takes_it_back_to_absent() {
    let state = testing::state();
    config_set(&state, &set(&[("ui.locale", serde_json::json!("zh"))])).expect("writes");
    config_set(&state, &unset(&["ui.locale"])).expect("removes");
    assert_eq!(
        config_get(&state).expect("a snapshot").values,
        serde_json::json!({})
    );
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

    let error = row_value(&row).expect_err("a corrupt row is refused");
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
        assert_eq!(row_value(&row).expect("parses"), value);
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

    assert_eq!(
        row_value(&good).expect("the good row still reads"),
        serde_json::json!("dark")
    );
    let error = row_value(&bad).expect_err("the bad row is still refused");
    assert_eq!(
        error.params.get("key").and_then(|value| value.as_str()),
        Some("ui.locale")
    );
}
