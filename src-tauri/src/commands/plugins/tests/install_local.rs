use super::*;

#[test]
fn installing_a_directory_records_it_switched_off() {
    // ★ The command an interface needs before it can offer an install at all. What it asserts is the answer and the
    // row: an install that reported success without writing one would look identical from the caller's side.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());

    let report = plugins_install(
        &state,
        &InstallSourceDto::Path {
            path: source.to_string_lossy().into_owned(),
        },
    )
    .expect("it installs");

    assert_eq!(report.plugin_id.as_str(), "@acme/hello");
    assert_eq!(report.version, "1.0.0");
    assert!(!report.enabled, "installing is not approving");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let row = state
        .db()
        .plugins()
        .expect("the rows")
        .into_iter()
        .find(|row| row.plugin_id == "@acme/hello")
        .expect("a row");
    assert!(!row.enabled, "installing must not switch it on");
    assert!(!row.trusted, "installing must not trust it");
    // ★ And the pipeline's one counter: the report says it worked, the recorder says the same. A success is the
    // `complete` stage — the refusal stages only appear when something stopped.
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("plugin_install_total{result=ok,stage=complete}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
}

#[test]
fn installing_a_package_file_uses_the_same_pipeline() {
    // ★ A `.synra` is the shape sync and the registry both produce, and a user can have one on disk. Passing it to the
    // directory-only path used to refuse with "not a directory" even though the unpacker and the installer were both
    // already there.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());
    let archive = root.path().join("hello.synra");
    synra_core::install::package_directory(&source, &archive).expect("the package is written");

    let report = plugins_install(
        &state,
        &InstallSourceDto::Path {
            path: archive.to_string_lossy().into_owned(),
        },
    )
    .expect("the package installs");

    assert_eq!(report.plugin_id.as_str(), "@acme/hello");
    assert_eq!(report.version, "1.0.0");
    assert!(!report.enabled);
    let installed = state.paths.plugin_dir("@acme/hello");
    assert!(
        installed.join("source.json").is_file(),
        "no source note in {installed:?}: {:?}",
        std::fs::read_dir(&installed).map(|entries| entries
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .collect::<Vec<_>>())
    );
    let note: serde_json::Value =
        serde_json::from_slice(&std::fs::read(installed.join("source.json")).expect("the note"))
            .expect("JSON");
    assert_eq!(
        std::fs::canonicalize(note["path"].as_str().expect("a path")).expect("canonical"),
        std::fs::canonicalize(&archive).expect("a canonical archive"),
        "the installed note names the package, not the unpacked temporary directory"
    );
}

#[test]
fn a_refused_install_is_counted_with_the_stage_that_refused_it() {
    // ★ The interesting half: a counter that only counts successes says "installs are fine" while every attempt is
    // refused at `source`. The stage is the refusal's own name, the same one its `IpcError` params carry.
    let (root, state) = testing::file_state();
    let state = with_engine(state);

    let error = install_directory(&state, &root.path().join("not-a-package")).expect_err("refused");

    assert_eq!(error.params["stage"], "source");
    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("plugin_install_total{result=refused,stage=source}"),
        Some(&synra_observe::metrics::Value::Counter(1))
    );
}
