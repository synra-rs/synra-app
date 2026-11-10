use super::*;

#[test]
fn the_actions_a_plugin_declared_are_listed_with_their_schema() {
    // ★ `args` is parsed rather than a JSON string inside JSON: the contract's `ActionDescriptor` declares it as
    // `unknown`, and a page rendering a form should not have to parse a string it was handed inside an object.
    let (_root, state, token, _backend) =
        action_page(synra_plugin_host::Scripted::Ok("null".to_owned()), "null");

    let answer = super::super::plugin_ui_rpc(&state, &token, "self.actions", &serde_json::json!({}))
        .expect("the list");

    assert_eq!(
        answer,
        serde_json::json!([{
            "name": "send",
            "args": {"type": "object"},
            "description": "Send a message",
        }])
    );
}

#[test]
fn an_action_runs_on_the_headless_half() {
    let (_root, state, token, backend) = action_page(
        synra_plugin_host::Scripted::Ok(r#"{"sent":true}"#.to_owned()),
        r#"{"to":"bob"}"#,
    );

    let answer = super::super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send", "args": {"to": "bob"}}),
    )
    .expect("the call");

    assert_eq!(answer, serde_json::json!({"sent": true}));
    assert_eq!(backend.calls(), 1, "the headless half was not run");
}

#[test]
fn an_action_this_plugin_never_declared_is_refused_before_the_call() {
    // ★ The ownership check the design asks for. `Work::InvokeAction` carries a name and nothing about who declared
    // it; without this lookup a page could name any action, which is one plugin driving another through a token it was
    // given for itself.
    let (_root, state, token, backend) =
        action_page(synra_plugin_host::Scripted::Ok("null".to_owned()), "null");

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "delete-everything"}),
    )
    .expect_err("that action is not this plugin's");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("action")
    );
    assert_eq!(
        backend.calls(),
        0,
        "an undeclared action reached the headless half"
    );
}

#[test]
fn a_structured_failure_keeps_the_code_the_plugin_chose() {
    let (_root, state, token, _backend) = action_page(
        synra_plugin_host::Scripted::Failed {
            code: "device_offline".to_owned(),
            message: "the other end is away".to_owned(),
            details: None,
        },
        "null",
    );

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send"}),
    )
    .expect_err("the plugin refused");

    assert_eq!(refused.code, ErrorCode::DeviceOffline);
    assert_eq!(
        refused
            .params
            .get("reason")
            .and_then(|value| value.as_str()),
        Some("the other end is away")
    );
}

#[test]
fn a_call_that_ran_out_of_time_is_a_plugin_timeout() {
    let (_root, state, token, _backend) = action_page(synra_plugin_host::Scripted::Timeout, "null");

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send"}),
    )
    .expect_err("the call timed out");

    assert_eq!(refused.code, ErrorCode::PluginTimeout);
}
