use super::*;

#[test]
fn a_bundle_is_written_where_it_says_it_is() {
    // ★ Because the path is the only part of the answer a user acts on.
    //
    // Returning a destination that does not exist produces a bundle that cannot be attached to a support ticket, and the
    // failure surfaces at the moment the user is least able to deal with it — after they have clicked "send".
    let root = testing::Root::fresh("diag-writes-where-it-says");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    assert!(
        std::path::Path::new(&result.path).is_file(),
        "the file it named is at {}",
        result.path
    );
}

#[test]
fn the_exclusions_are_written_into_the_file_not_only_returned() {
    // ★ The promise that is easy to get wrong.
    //
    // Returning the exclusion list to the screen is not enough: once the file is sent, the screen is gone. A bundle whose
    // own contents do not say what was left out relies on the recipient trusting whoever sent it.
    let root = testing::Root::fresh("diag-exclusions");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    let excluded = file
        .get("excluded")
        .and_then(Value::as_array)
        .expect("the file carries the list");
    assert!(
        !excluded.is_empty(),
        "and it is not empty — the defaults exclude the audit trail"
    );
    assert_eq!(
        excluded,
        &result
            .excluded
            .iter()
            .map(|section| Value::String(section.clone()))
            .collect::<Vec<Value>>(),
        "what the screen showed and what the file says are the same list"
    );
}

#[test]
fn a_default_bundle_leaves_out_the_audit_trail_and_the_log() {
    // ★ Why the defaults are off.
    //
    // The audit trail names every plugin the user runs and the debug log is where a token that slipped through ends up.
    // Neither is included unless the user asked, and "asked" has to mean the option rather than the fact that a source
    // could be produced.
    let root = testing::Root::fresh("diag-defaults");
    let state = testing::state_in(&root);

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    assert!(
        is_empty_section(&file, "auditTrail"),
        "no audit trail without consent"
    );
    assert!(is_empty_section(&file, "logs"), "and no log");
    assert!(
        !result
            .included
            .iter()
            .any(|section| section == "audit-trail"),
        "and the consent screen said so before the file was written: {:?}",
        result.included
    );
}

#[test]
fn asking_for_the_audit_trail_puts_an_empty_one_in_rather_than_pretending_it_ran() {
    // ★ The choice, stated: the section is present rather than omitted.
    //
    // A missing section and an empty one read the same on a screen, and the first is indistinguishable from "this bundle
    // came from a version that never had one". So the section is always there when it was asked for.
    //
    // ★ What this test does **not** cover, said plainly rather than implied.
    //
    // It cannot tell a section wired to the plugin set's ring from one still stubbed to `Some(Vec::new())` — both are
    // empty here, because this fixture has no plugin engine, so no instance ran and no call was authorised. Catching that
    // needs an audit entry, and an entry is produced when a **guest makes a host call**: the scripted backend never
    // enters a guest, so no fixture in this repository can produce one. The same missing toolchain (`wasm32-wasip2`)
    // that leaves `synra-plugin-host`'s two guest tests `#[ignore]`d leaves this seam unguarded.
    //
    // The binding is a one-line read of `state.plugin_set()`, and this comment is where the next person finds out that
    // nothing would go red if it were removed.
    let root = testing::Root::fresh("diag-audit");
    let state = testing::state_in(&root);

    let result = diag_export(
        &state,
        &synra_ipc::DiagExportOptions {
            include_audit: true,
            ..options()
        },
    )
    .expect("exports");
    let file = bundle(&result);
    assert_eq!(
        file.get("auditTrail")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "present, and empty because nothing has run"
    );
    assert!(
        result
            .included
            .iter()
            .any(|section| section == "audit-trail"),
        "and listed as included: {:?}",
        result.included
    );

    // And absent when it was not asked for, which is the half this test *can* check.
    let result = diag_export(
        &state,
        &synra_ipc::DiagExportOptions {
            include_audit: false,
            ..options()
        },
    )
    .expect("exports");
    assert!(
        !result
            .included
            .iter()
            .any(|section| section == "audit-trail"),
        "not asked for, so not in the bundle: {:?}",
        result.included
    );
}

#[test]
fn the_plugin_section_reports_what_the_set_knows_rather_than_a_constant() {
    // ★ The three fields that used to be constants.
    //
    // `had_refusals: false`, `active: false` and `version: String::new()` were written into every bundle, and the first
    // of them is a **claim**: it tells a maintainer nothing was ever refused, in the document a user sends when they are
    // asking why something did not work.
    //
    // ★ What this covers and what it does not. The fixture has no plugin engine, so the set answers "not loaded, no
    // version, nothing refused" for every installed plugin — which is the truth about a shell with no engine. So this
    // asserts the *wiring* in one direction: a bundle made here says `active: false` because nothing is loaded, not
    // because the field is a literal. It cannot tell the two apart, and that seam is recorded where the read happens
    // (`loaded_plugins`), for the same reason the audit one is: producing a loaded plugin needs a guest that calls a host
    // function, and there is no toolchain for that in this repository.
    let root = testing::Root::fresh("diag-plugins");
    let state = testing::state_in(&root);
    // Installed so the section has a row to describe; a bundle with no plugins in it would make the assertions vacuous.
    state
        .db()
        .insert_plugin(
            "@acme/notes",
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");

    let result = diag_export(&state, &options()).expect("exports");
    let file = bundle(&result);
    let plugins = file
        .get("plugins")
        .and_then(Value::as_array)
        .expect("a plugin section");

    // The fixture installs plugins, and each one is reported — rather than the section being empty, which would make the
    // assertions below vacuous.
    assert_eq!(plugins.len(), 1, "the fixture installs one plugin: {file}");
    for plugin in plugins {
        assert_eq!(plugin.get("active").and_then(Value::as_bool), Some(false));
        assert_eq!(
            plugin.get("hadRefusals").and_then(Value::as_bool),
            Some(false)
        );
        assert!(
            plugin.get("version").is_some(),
            "the key is present even when the value is empty"
        );
    }
}
