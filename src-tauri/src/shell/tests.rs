use std::str::FromStr as _;

#[test]
fn the_wake_shortcut_parses_before_it_is_registered() {
    // Registering a string the platform cannot parse fails at runtime with the window already gone; parsing it here
    // turns a typo into a failing test.
    assert!(
        tauri_plugin_global_shortcut::Shortcut::from_str(super::WAKE_SHORTCUT).is_ok(),
        "{} is not a shortcut",
        super::WAKE_SHORTCUT
    );
}
