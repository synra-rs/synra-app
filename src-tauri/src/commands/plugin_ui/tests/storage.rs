use super::*;

// ── Storage ──────────────────────────────────────────────────────────────────────────────────────

/// A state with the plugin installed (the storage table's foreign key needs the row) and a page holding both storage
/// capabilities.
fn storage_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    state
        .db()
        .insert_plugin(
            plugin_id().as_str(),
            true,
            true,
            "installed",
            crate::testing::FIXED_NOW_MS,
            crate::testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    let token = hosted_page_with(&state, &["storage:local:read", "storage:local:write"]);
    (root, state, token)
}

#[test]
fn storage_round_trips_through_the_channel() {
    let (_root, state, token) = storage_page();

    let written = super::super::plugin_ui_rpc(
        &state,
        &token,
        "storage.set",
        &serde_json::json!({"key": "draft", "value": "hello"}),
    )
    .expect("a write");
    assert_eq!(written, serde_json::Value::Null);

    assert_eq!(
        super::super::plugin_ui_rpc(
            &state,
            &token,
            "storage.get",
            &serde_json::json!({"key": "draft"})
        )
        .expect("a read"),
        serde_json::json!("hello")
    );
    // An absent key is `null`, the client's `string | null` — not an error and not an empty string.
    assert_eq!(
        super::super::plugin_ui_rpc(
            &state,
            &token,
            "storage.get",
            &serde_json::json!({"key": "absent"})
        )
        .expect("a read"),
        serde_json::Value::Null
    );
    assert_eq!(
        super::super::plugin_ui_rpc(&state, &token, "storage.keys", &serde_json::json!({})).expect("keys"),
        serde_json::json!(["draft"])
    );
    assert_eq!(
        super::super::plugin_ui_rpc(&state, &token, "storage.quota", &serde_json::json!({}))
            .expect("quota"),
        serde_json::json!({"usedBytes": 5, "maxBytes": 64 * 1024 * 1024})
    );

    assert_eq!(
        super::super::plugin_ui_rpc(
            &state,
            &token,
            "storage.delete",
            &serde_json::json!({"key": "draft"})
        )
        .expect("a delete"),
        serde_json::json!(true)
    );
    assert_eq!(
        super::super::plugin_ui_rpc(
            &state,
            &token,
            "storage.get",
            &serde_json::json!({"key": "draft"})
        )
        .expect("a read"),
        serde_json::Value::Null
    );
}

#[test]
fn a_storage_call_without_the_capability_is_refused() {
    // ★ The gate, not the façade's `env.can`: a page that somehow called the method directly still reaches this check.
    let (_root, state) = crate::testing::file_state();
    state
        .db()
        .insert_plugin(
            plugin_id().as_str(),
            true,
            true,
            "installed",
            crate::testing::FIXED_NOW_MS,
            crate::testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    let read_only = hosted_page_with(&state, &["storage:local:read"]);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &read_only,
        "storage.set",
        &serde_json::json!({"key": "k", "value": "v"}),
    )
    .expect_err("no write capability");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused
            .params
            .get("capability")
            .and_then(|value| value.as_str()),
        Some("storage:local:write")
    );
    assert_eq!(
        state
            .db()
            .storage_get(plugin_id().as_str(), "k")
            .expect("a read"),
        None,
        "a refused write left something behind"
    );
}

#[test]
fn a_key_that_is_not_a_string_is_an_argument_error() {
    let (_root, state, token) = storage_page();

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "storage.get",
        &serde_json::json!({"key": 7}),
    )
    .expect_err("a key is a string");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("key")
    );
}

#[test]
fn a_write_replaces_rather_than_double_counting_the_old_value() {
    // ★ The arithmetic a real 64 MiB ceiling cannot exercise: rewriting a key with a slightly larger value must not
    // count the value it replaces twice.
    assert_eq!(super::super::storage_after(10, 4, 6), 12);
    assert_eq!(
        super::super::storage_after(10, 8, 3),
        5,
        "a smaller replacement shrinks the store"
    );
    assert_eq!(super::super::storage_after(0, 0, 0), 0);
    // A stored value larger than the measured usage cannot happen through the store, but saturating keeps a corrupt
    // measurement from wrapping into a tiny number and letting an oversized write through.
    assert_eq!(super::super::storage_after(2, 9, 5), 5);
}
