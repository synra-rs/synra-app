use super::*;
use crate::testing::file_state;
use serde_json::json;

fn source(state: &AppState, address: &str) -> Base {
    state
        .db()
        .set_config(REGISTRY_KEY, &json!(address), 1)
        .expect("source");
    bases(state).expect("configured source").remove(0)
}

#[test]
fn a_source_does_not_grant_plain_http_to_remote_artifacts() {
    let (_root, state) = file_state();
    let base = source(&state, "static:http://localhost:4123");
    let url = "http://unresolvable.invalid/plugin.tgz";
    let error = artifact_client(&base, url)
        .expect("a guarded client")
        .fetch(&get(url))
        .expect_err("scheme checked before DNS");
    assert_eq!(error.code, ErrorCode::SchemeNotAllowed);
    let base = source(&state, "static:https://bucket.example");
    let error =
        artifact_client(&base, "http://127.0.0.1/plugin.tgz").expect_err("no HTTP downgrade");
    assert_eq!(error.code, ErrorCode::SchemeNotAllowed);
}

#[test]
fn an_explicit_local_source_can_download_a_query_bearing_artifact() {
    let (address, server) = super::test_server::respond(200, "ok");
    let (_root, state) = file_state();
    let base = source(&state, &format!("static:{address}"));
    let url = format!("{address}/plugin.tgz?signature=fixture");
    let response = artifact_client(&base, &url)
        .expect("client")
        .fetch(&get(&url))
        .expect("local fetch");
    assert_eq!(response.body, "ok");
    let request = server.join().expect("server completed");
    assert!(request.starts_with("GET /plugin.tgz?signature=fixture HTTP/1.1"));
}
