use super::*;

#[test]
fn the_exclusions_are_about_what_can_never_be_included_not_about_what_was_declined() {
    // ★ What `excluded` actually means, pinned because it is easy to assume otherwise.
    //
    // It is the standing policy list — clipboard, message bodies, file contents, passwords, a plugin's secure storage —
    // and it is the same whatever the user chose. Reading it as "what this user did not share" would make every bundle
    // look like it had declined something, and would hide the part that matters: none of it can ever be in one.
    let root = testing::Root::fresh("diag-exclusion-meaning");
    let state = testing::state_in(&root);

    let lean = diag_export(&state, &options()).expect("exports");
    let generous = diag_export(
        &state,
        &synra_ipc::DiagExportOptions {
            include_audit: true,
            ..options()
        },
    )
    .expect("exports");

    assert_eq!(
        lean.excluded, generous.excluded,
        "consenting to more does not change the standing policy"
    );
    assert!(
        lean.excluded
            .iter()
            .any(|section| section.contains("pairing secrets")),
        "and the policy does name what it is: {:?}",
        lean.excluded
    );
}

#[test]
fn the_configuration_in_the_bundle_is_what_the_settings_screen_shows() {
    let root = testing::Root::fresh("diag-config");
    let state = testing::state_in(&root);
    crate::commands::config_set(
        &state,
        &synra_ipc::ConfigPatch {
            set: [("ui.locale".to_owned(), serde_json::json!("zh"))]
                .into_iter()
                .collect(),
            unset: Vec::new(),
        },
    )
    .expect("writes a setting");

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    assert_eq!(
        file.get("config")
            .and_then(|config| config.get("ui.locale")),
        Some(&serde_json::json!("zh")),
        "a maintainer reading this can see what the user had set"
    );
}

#[test]
fn the_bundle_names_the_versions_a_mismatch_would_be_found_in() {
    // ★ Both versions, and why both.
    //
    // A protocol mismatch between an app and a kernel is the single most common thing a bundle is sent for, and it is
    // invisible from either side alone.
    let root = testing::Root::fresh("diag-versions");
    let state = testing::state_in(&root);

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let environment = file.get("environment").expect("the environment is there");
    assert_eq!(
        environment.get("kernelVersion").and_then(Value::as_str),
        Some(crate::KERNEL_VERSION)
    );
    assert_eq!(
        environment.get("appVersion").and_then(Value::as_str),
        Some(crate::KERNEL_VERSION)
    );
    assert_eq!(
        environment.get("os").and_then(Value::as_str),
        Some(std::env::consts::OS)
    );
}

#[test]
fn a_webview_version_is_absent_rather_than_guessed() {
    // ★ `None`, not a placeholder.
    //
    // This library does not touch a window, so it cannot know the WebView's version. Writing "unknown" would be a value,
    // and a maintainer would spend a day on it.
    let root = testing::Root::fresh("diag-webview");
    let state = testing::state_in(&root);

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let webview = file
        .get("environment")
        .and_then(|environment| environment.get("webviewVersion"));
    assert!(
        webview.is_none() || webview == Some(&Value::Null),
        "absent or null, never invented"
    );
}

#[test]
fn a_paired_device_appears_in_the_bundle_by_id() {
    let root = testing::Root::fresh("diag-devices");
    let state = testing::state_in(&root);
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: "44444444-4444-4444-8444-444444444444".to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            address: None,
            paired_at_ms: 1,
        })
        .expect("pairs");

    let file = bundle(&diag_export(&state, &options()).expect("exports"));
    let devices = file
        .get("devices")
        .and_then(Value::as_array)
        .expect("the devices section is there");
    assert_eq!(devices.len(), 1);
    assert_eq!(
        devices[0].get("deviceId").and_then(Value::as_str),
        Some("44444444-4444-4444-8444-444444444444"),
        "so a maintainer can tell which machine was paired"
    );
}

#[test]
fn two_exports_in_one_session_are_two_files_and_not_one_overwritten() {
    // ★ The comparison this makes possible.
    //
    // Exporting twice is how a user shows "this started after I changed X" — so the two bundles have to both survive. A
    // name built from the second alone silently replaces the first, and the user compares a bundle against itself and
    // concludes nothing changed.
    let root = testing::Root::fresh("diag-twice");
    let state = testing::state_in(&root);

    let first = diag_export(&state, &options()).expect("first");
    let second = diag_export(&state, &options()).expect("second");

    assert_ne!(
        first.path, second.path,
        "the second export is not the first one again"
    );
    assert!(
        std::path::Path::new(&first.path).is_file(),
        "and the first is still there"
    );
    assert!(std::path::Path::new(&second.path).is_file());
}

#[test]
fn the_exclusions_are_the_same_whether_the_user_shared_more_or_less() {
    let root = testing::Root::fresh("diag-exclusion-constancy");
    let state = testing::state_in(&root);
    let first = diag_export(&state, &options()).expect("exports");
    let second = diag_export(
        &state,
        &synra_ipc::DiagExportOptions {
            include_audit: true,
            ..options()
        },
    )
    .expect("exports");
    assert_eq!(first.excluded, second.excluded);
}
