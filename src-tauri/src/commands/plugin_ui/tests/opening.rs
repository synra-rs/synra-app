use super::*;

#[test]
fn opening_a_page_records_who_it_belongs_to_and_where_it_lives() {
    // Tauri must receive a parseable origin and the actual entry file, including its nested directory.
    let (_root, state) = installed_with_ui();

    let request = opened(&state, ID);

    let parsed = url::Url::parse(&request.url).expect("a real URL");
    assert_eq!(parsed.as_str(), request.url);
    assert_eq!(parsed.path(), "/dist/index.html");
    assert_eq!(
        super::super::split_ui_url(
            parsed.as_str(),
            &[synra_protocol::PluginId::parse(ID).unwrap()]
        )
        .unwrap()
        .1,
        "dist/index.html"
    );
    assert!(
        request.script.contains(&request.label),
        "the script names the page it is for"
    );

    let entry = state
        .hosted_ui
        .lock()
        .get(&request.label)
        .cloned()
        .expect("the page is recorded");
    assert_eq!(entry.plugin_id.as_str(), ID);
    assert_eq!(entry.version, "1.0.0");
    assert!(!entry.ready, "it has not said it has drawn yet");
}

#[test]
fn opening_a_page_issues_the_grants_the_user_allowed() {
    // ★ The page's permissions are the decisions the user made, not an empty set. `open_request` used to issue with
    // `CapabilitySet::new()` — so every page was powerless no matter what it had been granted, and `env.can` would answer
    // false for a capability the user had approved.
    let (_root, state) = installed_with_ui();
    state
        .db()
        .record_grants(
            ID,
            &[
                ("log:*".to_owned(), true),
                ("network:fetch".to_owned(), false),
            ],
            crate::testing::FIXED_NOW_MS,
            "sha256-manifest",
        )
        .expect("decisions");
    let request = opened(&state, ID);
    let entry = state
        .hosted_ui
        .lock()
        .get(&request.label)
        .cloned()
        .expect("recorded");
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();

    let session = state
        .ui_sessions
        .validate(
            entry.nonce,
            &synra_protocol::ids::PluginId::parse(ID).expect("an id"),
            "1.0.0",
            now,
        )
        .expect("a live session");

    let allowed = synra_capability::Capability::parse("log:write").expect("a capability");
    let refused = synra_capability::Capability::parse("network:fetch").expect("a capability");
    assert!(
        session.grants().contains(&allowed),
        "a granted family did not reach the session"
    );
    assert!(
        !session.grants().contains(&refused),
        "a refused capability reached the session"
    );
}

#[test]
fn opening_a_page_for_a_plugin_that_is_not_installed_is_refused() {
    // ★ The refusal a caller can act on: "install it".
    let state = crate::testing::state();

    let refused = super::super::open_request(&state, "@acme/absent").expect_err("nothing is installed");

    assert_eq!(refused.code, ErrorCode::PluginNotInstalled);
    assert!(
        state.hosted_ui.lock().is_empty(),
        "and nothing was recorded"
    );
}

#[test]
fn opening_a_page_for_a_plugin_with_no_ui_is_a_different_refusal() {
    // ★ The other refusal, and a different instruction: it is installed, so "install it" would be wrong advice. It has no
    // window to open, and nothing should be recorded or issued for a page that will never exist.
    let (_root, state) = installed_headless_only();

    let refused = super::super::open_request(&state, ID).expect_err("it has no UI");

    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert!(
        state.hosted_ui.lock().is_empty(),
        "and no page was recorded"
    );
    assert!(state.ui_sessions.is_empty(), "nor a session issued");
}
