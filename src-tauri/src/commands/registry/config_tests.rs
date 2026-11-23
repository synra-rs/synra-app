use super::*;
use crate::{commands::config::config_set, testing::file_state};
use serde_json::json;

fn query() -> CatalogQuery {
    CatalogQuery {
        query: "reader".to_owned(),
        registry: None,
        limit: None,
        offset: None,
        sort: None,
    }
}

#[test]
fn missing_and_invalid_settings_have_distinct_typed_reasons() {
    let (_root, state) = file_state();
    for value in [
        None,
        Some(json!("")),
        Some(json!([])),
        Some(json!([123, "https://valid.example"])),
    ] {
        match value.as_ref() {
            Some(value) => state
                .db()
                .set_config(REGISTRY_KEY, value, 1)
                .expect("stored"),
            None => {
                state.db().unset_config(REGISTRY_KEY).expect("unset");
            }
        }
        let error = search(&state, &query()).expect_err("no network request");
        let detail = error.details.expect("typed details");
        let expected = if value
            .as_ref()
            .is_some_and(|value| value == &json!([123, "https://valid.example"]))
        {
            "invalidConfiguration"
        } else {
            "notConfigured"
        };
        assert_eq!(detail["registry"]["kind"], expected);
        if expected == "invalidConfiguration" {
            assert_eq!(detail["registry"]["index"], 0);
        }
    }
}

#[test]
fn an_invalid_registry_prevents_every_other_setting_in_the_patch_from_writing() {
    let (_root, state) = file_state();
    for value in [
        json!(123),
        json!({}),
        json!(["https://valid.example", 123]),
        json!("https://host:bad"),
    ] {
        let patch = synra_ipc::ConfigPatch {
            set: [
                ("ui.locale".to_owned(), json!("zh-CN")),
                (REGISTRY_KEY.to_owned(), value),
            ]
            .into_iter()
            .collect(),
            unset: Vec::new(),
        };
        let error = config_set(&state, &patch).expect_err("refused patch");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(state.db().config().expect("read configuration").is_empty());
    }
}

#[test]
fn a_static_source_is_configured_but_has_no_search_endpoint() {
    let (_root, state) = file_state();
    state
        .db()
        .set_config(
            REGISTRY_KEY,
            &json!("static:https://bucket.example/plugins"),
            1,
        )
        .expect("static source");
    let error = search(&state, &query()).expect_err("static search");
    assert_eq!(
        error.details,
        Some(json!({"registry": {"kind": "searchUnsupported"}}))
    );
    let parsed = bases(&state).expect("valid configuration");
    assert_eq!(parsed[0].url(), "https://bucket.example/plugins");
    assert_eq!(parsed[0].kind(), Kind::Static);
}

#[test]
fn configuration_changes_preserve_the_current_mirror_order_and_ipv6_authority() {
    let (_root, state) = file_state();
    for values in [
        json!(["https://first.example", "http://[::1]:4123"]),
        json!(["https://next.example"]),
    ] {
        state
            .db()
            .set_config(REGISTRY_KEY, &values, 1)
            .expect("sources");
        let actual: Vec<_> = bases(&state)
            .expect("parse sources")
            .into_iter()
            .map(|base| base.url().to_owned())
            .collect();
        assert_eq!(serde_json::to_value(actual).unwrap(), values);
    }
}

#[test]
fn local_update_detection_does_not_treat_mirror_arrays_or_corrupt_settings_as_absent() {
    let (_root, state) = file_state();
    let source = state.paths.cache_dir().join("local-source");
    let plugin = state.paths.plugin_dir("@acme/local");
    std::fs::create_dir_all(&source).expect("local directory");
    std::fs::create_dir_all(&plugin).expect("installed directory");
    std::fs::write(
        plugin.join("source.json"),
        json!({"path": source}).to_string(),
    )
    .expect("source note");
    for (value, local) in [
        (None, true),
        (Some(json!("")), true),
        (Some(json!([])), true),
        (Some(json!("https://registry.example")), false),
        (Some(json!(["https://registry.example"])), false),
        (Some(json!([123, "https://registry.example"])), false),
        (Some(json!({})), false),
    ] {
        match value.as_ref() {
            Some(value) => state
                .db()
                .set_config(REGISTRY_KEY, value, 1)
                .expect("setting"),
            None => {
                state.db().unset_config(REGISTRY_KEY).expect("unset");
            }
        }
        assert_eq!(
            crate::commands::plugins::local_directory_source(&state, "@acme/local").is_some(),
            local
        );
    }
}
