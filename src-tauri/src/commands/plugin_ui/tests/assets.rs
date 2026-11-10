#[test]
fn a_plugin_address_splits_at_the_id_the_host_opened() {
    // Scoped package names retain their complete identity after standard URL parsing.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::super::split_ui_url(
        &super::super::origin::entry(&ids[0], "index.html", None).unwrap(),
        &ids,
    )
    .expect("it is one of ours");

    assert_eq!(id.as_str(), "@acme/hello");
    assert_eq!(path, "index.html");
}

#[test]
fn packages_with_a_shared_name_prefix_have_separate_origins() {
    // A package name prefix never grants access to another package's files.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::super::split_ui_url(
        &super::super::origin::entry(&ids[1], "app.js", None).unwrap(),
        &ids,
    )
    .expect("it is one of ours");

    assert_eq!(id.as_str(), "@acme/hello-world");
    assert_eq!(path, "app.js");
}

#[test]
fn an_address_for_an_unknown_plugin_is_not_split() {
    let ids = [synra_protocol::ids::PluginId::parse("@acme/hello").expect("an id")];

    for url in [
        "synra-plugin://@acme/other/index.html",
        "https://example.com/x",
        "synra-plugin://@acme/hello",
    ] {
        assert!(
            super::super::split_ui_url(url, &ids).is_none(),
            "{url} must not be served"
        );
    }
}

#[test]
fn a_bundle_file_is_served_with_its_type() {
    use std::path::Path;

    let root = std::env::temp_dir().join(format!("synra-ui-{}", std::process::id()));
    std::fs::create_dir_all(root.join("assets")).expect("a bundle");
    std::fs::write(root.join("index.html"), b"<!doctype html>").expect("a page");
    std::fs::write(root.join("assets/app.js"), b"export {}").expect("a script");

    let (bytes, content_type) = super::super::ui_file(&root, "index.html").expect("it is there");

    assert_eq!(content_type, "text/html; charset=utf-8");
    assert_eq!(bytes, b"<!doctype html>");
    let (_, script_type) = super::super::ui_file(&root, "assets/app.js").expect("it is there");
    assert!(
        script_type.starts_with("text/javascript"),
        "got {script_type}"
    );

    let _ = std::fs::remove_dir_all(Path::new(&root));
}

#[test]
fn a_path_that_leaves_the_bundle_is_refused() {
    // ★ The assertion a security review asks for. `../../identity.toml` is the request that would hand a plugin the
    // device's private key, and both guards have to say no: the artifact rules, and the containment check after the join.
    let root = std::env::temp_dir().join(format!("synra-ui-escape-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("a bundle");

    for path in ["../identity.toml", "../../etc/passwd", "/etc/passwd"] {
        let refused = super::super::ui_file(&root, path).expect_err("{path} must not be served");
        assert_eq!(
            refused.code,
            synra_protocol::ErrorCode::InvalidArgument,
            "{path}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_initialization_script_keeps_the_seed_in_the_wrapper_and_calls_the_facade() {
    // ★ Three properties in one string:
    //   * the token is quoted, because `u128::MAX` is not representable as a JavaScript number and a bare one would
    //     round — the page would present a token that validates for nobody;
    //   * the seed is the **wrapper function's argument**, not a global, so a script on the page cannot read the token
    //     or rewrite its own grants;
    //   * the facade is inline rather than imported, because an initialization script has no module loader.
    let seed = super::super::InjectionSeed {
        label: "page-1",
        token: u128::MAX.to_string(),
        plugin_id: "@acme/hello",
        granted: &["log:*".to_owned()],
        denied_capabilities: &[],
        locale: "zh-CN",
        dark_mode: true,
        sdk_version: "0.1.0",
        platform: "linux",
        host_kind: "desktop",
        app_version: "9.9.9",
        platform_capabilities: vec!["open_url".to_owned()],
    };

    let script = super::super::initialization_script("/* facade */", &seed).expect("a script");

    assert!(
        script.starts_with("(function(seed){"),
        "the seed is a parameter of the wrapper: {script}"
    );
    assert!(script.contains("\"label\":\"page-1\""), "the label, quoted");
    assert!(
        script.contains(&format!("\"token\":{:?}", u128::MAX.to_string())),
        "the token as a **string**, so no digits are lost"
    );
    assert!(
        script.contains("\"granted\":[\"log:*\"]"),
        "the decisions travel with the seed"
    );
    assert!(
        script.contains("\"platformCapabilities\":[\"open_url\"]"),
        "the platform's answer travels with the seed: {script}"
    );
    assert!(script.contains("/* facade */"), "the facade is inline");
    assert!(
        script.contains("SynraInjection.boot(seed);"),
        "and it is called with the seed"
    );
    assert!(
        !script.contains("__SYNRA_PLUGIN__"),
        "the seed must not be reachable through a global: {script}"
    );
}

#[test]
fn a_page_may_only_stay_on_itself() {
    // ★ `has_ui` says the plugin has a page; this says what the page may do with the window. A fragment is the same
    // document and is how a single-page plugin routes; every other destination changes what is shown while keeping the
    // token the window was opened with.
    let opened = "synra-plugin://acme/hello/index";

    assert!(super::super::stays_on_page(opened, opened));
    assert!(super::super::stays_on_page(
        "synra-plugin://acme/hello/index#/settings",
        opened
    ));
    assert!(super::super::stays_on_page(
        "synra-plugin://acme/hello/index#",
        opened
    ));

    // Another page of the same bundle is still a navigation, and would keep this page's authority.
    assert!(!super::super::stays_on_page(
        "synra-plugin://acme/hello/other",
        opened
    ));
    // The prefix trap: a page whose address merely starts with the opened one.
    assert!(!super::super::stays_on_page(
        "synra-plugin://acme/hello/index2",
        opened
    ));
    // Another plugin's origin, and the schemes a page might use to leave the app entirely.
    assert!(!super::super::stays_on_page(
        "synra-plugin://acme/other/index",
        opened
    ));
    assert!(!super::super::stays_on_page("https://example.com/", opened));
    assert!(!super::super::stays_on_page("file:///etc/passwd", opened));
    assert!(!super::super::stays_on_page("data:text/html,hello", opened));
    assert!(!super::super::stays_on_page("", opened));
}

#[cfg(feature = "tauri-shell")]
#[test]
fn a_plugin_page_is_served_with_a_policy_that_cannot_reach_the_network() {
    // ★ The design assigns the CSP to the protocol handler (08 §550). Without this header a page's network access depends
    // on a global setting the plugin cannot see. Only Tauri's local IPC transport may connect directly.
    let response = super::super::document_response(b"<html></html>".to_vec(), "text/html; charset=utf-8");

    assert_eq!(response.status(), 200);
    let policy = response
        .headers()
        .get("content-security-policy")
        .expect("the policy header")
        .to_str()
        .expect("a header string");

    assert!(policy.contains("default-src 'none'"), "{policy}");
    let connect = policy
        .split(';')
        .map(str::trim)
        .find_map(|directive| directive.strip_prefix("connect-src "))
        .expect("an explicit connection policy");
    assert_eq!(
        connect.split_whitespace().collect::<Vec<_>>(),
        ["ipc:", "https://ipc.localhost"],
        "no general network or plugin-origin connection allowance"
    );
    assert!(policy.contains("script-src 'self'"), "{policy}");
    assert!(policy.contains("form-action 'none'"), "{policy}");
}
