use super::*;

#[test]
fn uninstalling_removes_the_row_and_keeps_the_data() {
    // ★ The default the design document asks for: a plugin the user may reinstall keeps its settings, and deleting
    // them unless asked would be the interface deciding what is replaceable.
    let (_root, state) = installed();
    let data = state.paths.plugin_sandbox_dir(ID);
    std::fs::create_dir_all(&data).expect("some plugin data");
    std::fs::write(data.join("cache.bin"), [7u8; 5]).expect("some bytes");

    let report = super::super::plugins_uninstall(&state, ID, false).expect("uninstalls");

    assert!(
        state
            .db()
            .plugins()
            .expect("rows")
            .iter()
            .all(|row| row.plugin_id != ID),
        "the row is gone"
    );
    assert!(
        !state.paths.plugin_dir(ID).exists(),
        "and the artifacts with it"
    );
    assert!(data.exists(), "but the data stays unless asked for");
    // ★ The report the design names and nothing returned: the artifact's bytes were freed, and the five bytes in
    // the sandbox were **not** — `data_kept` is the same fact in a sentence the page can print.
    assert!(report.data_kept, "the data stayed");
    assert_eq!(
        report.bytes_freed as usize,
        MANIFEST.len(),
        "only the artifact was freed"
    );
    assert!(
        report.leftovers.is_empty(),
        "a wholesale removal leaves nothing behind"
    );
}

#[test]
fn asking_for_the_data_removes_it_as_well() {
    // ★ The other half of the `remove_data` parameter, which the page never sends — and which nothing tested. It is the
    // one value of it that cannot be undone, so an untested `true` is the worst one to leave untested.
    let (_root, state) = installed();
    let data = state.paths.plugin_sandbox_dir(ID);
    std::fs::create_dir_all(&data).expect("some plugin data");
    std::fs::write(data.join("cache.bin"), [7u8; 5]).expect("some bytes");

    let report = super::super::plugins_uninstall(&state, ID, true).expect("uninstalls");

    assert!(!data.exists(), "the data went with the plugin");
    assert!(!report.data_kept, "and the report says so");
    assert_eq!(
        report.bytes_freed as usize,
        MANIFEST.len() + 5,
        "artifact and sandbox both counted"
    );
}

#[test]
fn uninstalling_something_that_is_not_installed_is_a_refusal() {
    // A typo and a no-op are different answers, and this one names the identifier so the user can see their own.
    let (_root, state) = installed();

    let refused =
        super::super::plugins_uninstall(&state, "@acme/typo", false).expect_err("nothing is there");

    assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
}

#[test]
fn uninstalling_takes_the_plugin_out_of_the_set() {
    // ★ A removed plugin whose compiled form stayed in the set could still answer a share, from an artifact the
    // user just deleted.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert_eq!(summary.len(), 1);
    let state = state.with_plugin_loads(summary);
    let id = PluginId::parse(ID).expect("an id");
    assert!(state.plugin_set().expect("an engine").lock().contains(&id));

    super::super::plugins_uninstall(&state, ID, false).expect("it is removed");
    assert!(
        !state.plugin_set().expect("an engine").lock().contains(&id),
        "the compiled form stayed behind"
    );
    assert_eq!(
        state.plugin_loads().len(),
        0,
        "the report still claims it is loaded"
    );
}
