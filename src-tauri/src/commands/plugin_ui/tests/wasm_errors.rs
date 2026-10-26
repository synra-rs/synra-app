//! Page RPC, real SQLite declarations, PluginSet and a real Wasm component share the failure.

use super::{action_page, action_page_on, plugin_id};
use std::sync::Arc;
use synra_plugin_host::engine::WasmtimeBackend;
use synra_plugin_host::{Backend, BackendKind};
use synra_protocol::ErrorCode;

fn data(text: &str) -> String {
    text.bytes().map(|byte| format!("\\{byte:02x}")).collect()
}

fn component(code: &str, details: Option<&str>) -> Vec<u8> {
    let code_data = data(code);
    let code_len = code.len();
    let flag = u8::from(details.is_some());
    let details = details.unwrap_or_default();
    let details_data = data(details);
    let details_len = details.len();
    wat::parse_str(format!(r#"
(component
  (type $error (record (field "code" string) (field "message" string) (field "details" (option string))))
  (type $outcome (variant (case "ok" string) (case "err" $error)))
  (core module $guest
    (memory (export "memory") 1)
    (data (i32.const 1000) "{code_data}")
    (data (i32.const 2000) "request refused")
    (data (i32.const 3000) "{details_data}")
    (global $next (mut i32) (i32.const 8192))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $next))
      (global.set $next (i32.add (local.get $ptr) (local.get 3)))
      (local.get $ptr))
    (func $answer (result i32)
      (i32.store8 (i32.const 128) (i32.const 1))
      (i32.store (i32.const 132) (i32.const 1000))
      (i32.store (i32.const 136) (i32.const {code_len}))
      (i32.store (i32.const 140) (i32.const 2000))
      (i32.store (i32.const 144) (i32.const 15))
      (i32.store8 (i32.const 148) (i32.const {flag}))
      (i32.store (i32.const 152) (i32.const 3000))
      (i32.store (i32.const 156) (i32.const {details_len}))
      (i32.const 128))
    (func (export "one") (param i32 i32) (result i32) (call $answer))
    (func (export "two") (param i32 i32 i32 i32) (result i32) (call $answer))
    (func (export "finish") (param i32)))
  (core instance $g (instantiate $guest))
  (func $activate (param "config-json" string) (result $outcome)
    (canon lift (core func $g "one") (memory (core memory $g "memory"))
      (realloc (core func $g "realloc")) (post-return (core func $g "finish"))))
  (func $invoke (param "name" string) (param "args-json" string) (result $outcome)
    (canon lift (core func $g "two") (memory (core memory $g "memory"))
      (realloc (core func $g "realloc")) (post-return (core func $g "finish"))))
  (instance $lifecycle
    (export "error" (type $error)) (export "outcome" (type $outcome))
    (export "activate" (func $activate)))
  (instance $actions
    (export "error" (type $error)) (export "outcome" (type $outcome))
    (export "invoke" (func $invoke)))
  (export "synra:plugin/lifecycle@1.0.0" (instance $lifecycle))
  (export "synra:plugin/actions@1.0.0" (instance $actions)))
"#)).expect("component fixture")
}

#[test]
fn real_wasm_failures_reach_the_page_with_optional_details_and_source_codes() {
    let backend = Arc::new(WasmtimeBackend::new(BackendKind::WasmtimeCranelift).expect("engine"));
    for (code, ipc_code, details, expected) in [
        ("hostFailed", ErrorCode::Internal, None, None),
        (
            "hostFailed",
            ErrorCode::Internal,
            Some(""),
            Some(serde_json::json!("")),
        ),
        (
            "hostFailed",
            ErrorCode::Internal,
            Some("null"),
            Some(serde_json::Value::Null),
        ),
        (
            "hostFailed",
            ErrorCode::Internal,
            Some("not-json"),
            Some(serde_json::json!("not-json")),
        ),
        (
            "hostFailed",
            ErrorCode::Internal,
            Some(r#"{"kind":"timeout","cleanupPending":true,"label":"中文🖱️"}"#),
            Some(serde_json::json!({"kind":"timeout","cleanupPending":true,"label":"中文🖱️"})),
        ),
        (
            "customRefusal",
            ErrorCode::Internal,
            Some(r#"{"code":"device_offline"}"#),
            Some(serde_json::json!({"code":"device_offline"})),
        ),
        (
            "device_offline",
            ErrorCode::DeviceOffline,
            Some("[1,true]"),
            Some(serde_json::json!([1, true])),
        ),
        ("notImplemented", ErrorCode::Unsupported, None, None),
    ] {
        let (_root, state, token) =
            action_page_on(Backend::new(backend.clone()), component(code, details));
        let before = state
            .plugin_set()
            .expect("plugins")
            .lock()
            .health_of(plugin_id().as_str());
        assert_eq!(before, Some(synra_plugin_host::Health::Good));
        let error = super::super::plugin_ui_rpc(
            &state,
            &token,
            "self.invoke",
            &serde_json::json!({"action":"send"}),
        )
        .expect_err("guest refusal");
        assert_eq!(error.code, ipc_code);
        assert_eq!(error.params["reason"], "request refused");
        let diagnostic = error.details.as_ref().expect("source diagnostics");
        assert_eq!(diagnostic["pluginCode"], code);
        assert_eq!(diagnostic.get("pluginDetails"), expected.as_ref());
        let after = state
            .plugin_set()
            .expect("plugins")
            .lock()
            .health_of(plugin_id().as_str());
        assert_eq!(before, after, "a reported refusal must not degrade health");
        let wire = serde_json::to_value(&error).expect("IPC error JSON");
        assert_eq!(wire["details"], *diagnostic);
    }
}

#[test]
fn direct_host_refusals_use_the_same_details_adapter() {
    let source = synra_plugin_host::CallError::new("rateLimited", "try later")
        .with_details(r#"{"retryAfterMs":250,"code":"device_offline"}"#);
    let error = super::super::call_error(&source);
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(error.params["reason"], "try later");
    assert_eq!(
        error.details,
        Some(serde_json::json!({
            "pluginCode":"rateLimited", "pluginDetails":{"retryAfterMs":250,"code":"device_offline"}
        }))
    );
}

#[test]
fn execution_limits_use_the_shared_plugin_error_diagnostics() {
    use synra_plugin_host::Scripted;
    for (outcome, code, kind) in [
        (Scripted::Timeout, ErrorCode::PluginTimeout, "timeout"),
        (
            Scripted::FuelExhausted,
            ErrorCode::PluginTrapped,
            "fuelExhausted",
        ),
        (
            Scripted::MemoryExceeded,
            ErrorCode::PluginTrapped,
            "memoryExceeded",
        ),
        (
            Scripted::Trap {
                detail: "out of bounds".to_owned(),
            },
            ErrorCode::PluginTrapped,
            "trap",
        ),
    ] {
        let (_root, state, token, backend) = action_page(outcome, "null");
        let error = super::super::plugin_ui_rpc(
            &state,
            &token,
            "self.invoke",
            &serde_json::json!({"action":"send"}),
        )
        .expect_err("execution failure");
        assert_eq!(error.code, code);
        assert_eq!(error.details.as_ref().expect("diagnostics")["kind"], kind);
        assert_eq!(backend.calls(), 1);
    }
}
