use super::*;

#[test]
fn opening_a_page_writes_the_environment_the_facade_answers_from() {
    // ★ The facts a plugin asks synchronously — platform, host kind, SDK version, locale, dark mode — are decided here,
    // before the page loads. A missing one would leave a namespace answering `undefined`, which is the failure class the
    // injected-surface tests exist to prevent.
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
    // The settings the user chose.
    super::super::super::config::config_set(
        &state,
        &synra_ipc::ConfigPatch {
            set: [
                ("ui.locale".to_owned(), serde_json::json!("zh-CN")),
                ("ui.theme".to_owned(), serde_json::json!("dark")),
            ]
            .into_iter()
            .collect(),
            unset: Vec::new(),
        },
    )
    .expect("settings");

    let request = opened(&state, ID);

    for expected in [
        "\"pluginId\":\"@synra-plugin/chat\"",
        "\"granted\":[\"log:*\"]",
        "\"deniedCapabilities\":[\"network:fetch\"]",
        "\"locale\":\"zh-CN\"",
        "\"darkMode\":true",
        "\"hostKind\":\"desktop\"",
        "\"appVersion\":\"",
        // ★ The platform's own answer, not a constant: `supported_actions` decides it, and on a desktop it always
        // includes opening a URL.
        "\"platformCapabilities\":[",
        "\"open_url\"",
    ] {
        assert!(
            request.script.contains(expected),
            "the seed is missing {expected}: {}",
            request.script
        );
    }
    assert!(request.script.contains(&format!(
        "\"sdkVersion\":\"{}\"",
        synra_plugin_host::SDK_VERSION
    )));
    // ★ And the façade itself is in the script, called with the seed. Without it the page would have a seed and no
    // client — which is the state this whole round exists to end.
    assert!(
        request.script.contains("SynraInjection.boot(seed);"),
        "the façade is called with the seed"
    );
    assert!(
        request.script.contains("window.synra"),
        "the façade publishes the client: {}",
        &request.script[..request.script.len().min(200)]
    );
    assert!(
        !request.script.contains("__SYNRA_PLUGIN__"),
        "the token must not be reachable through a global"
    );
}

#[test]
fn a_platform_action_is_named_the_way_the_client_names_it() {
    // ★ Two vocabularies, on purpose: `platform:read-clipboard` is the permission the manifest declares, and
    // `clipboard_read` is what `env.platformSupports` takes. Passing one where the other belongs would answer `false`
    // for every action on a host that supports all of them — the exact bug this mapping exists to prevent.
    use synra_platform::PlatformAction as Action;

    assert_eq!(
        super::super::platform_capability(Action::OpenUrl),
        Some("open_url")
    );
    assert_eq!(
        super::super::platform_capability(Action::OpenPath),
        Some("open_path")
    );
    assert_eq!(
        super::super::platform_capability(Action::RevealPath),
        Some("reveal_path")
    );
    assert_eq!(
        super::super::platform_capability(Action::SimulateKeys),
        Some("simulate_keys")
    );
    assert_eq!(
        super::super::platform_capability(Action::ReadClipboard),
        Some("clipboard_read")
    );
    assert_eq!(
        super::super::platform_capability(Action::WriteClipboard),
        Some("clipboard_write")
    );
    assert_eq!(super::super::platform_capability(Action::Notify), Some("notify"));
}
