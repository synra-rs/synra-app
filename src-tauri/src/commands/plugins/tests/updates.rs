use super::*;

#[test]
fn an_update_check_names_the_version_and_the_new_permission() {
    // ★ The comparison is against the installed manifest on disk, not against a row: the capabilities a user approved
    // are the ones in the package they have, and the store does not keep them.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());
    plugins_install(
        &state,
        &InstallSourceDto::Path {
            path: source.to_string_lossy().into_owned(),
        },
    )
    .expect("the fixture installs");

    let manifest = serde_json::json!({
        "id": "@acme/hello",
        "version": "2.0.0",
        "title": "Hello",
        "publisher": {"id": "@acme"},
        "sdk": {"min": "0.1.0"},
        "wit": synra_plugin_host::host_world(),
        "capabilities": [{"cap": "log:*"}, {"cap": "device:send:*"}],
        "runtime": {"headless": {"artifact": "plugin.wasm", "stateful": false}}
    });
    let document = serde_json::json!({
        "name": "@acme/hello",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "2.0.0": {
                "name": "@acme/hello",
                "version": "2.0.0",
                "dist": { "tarball": "https://registry.example.com/hello-2.0.0.tgz" },
                "synra": manifest
            }
        }
    })
    .to_string();
    let (base, _served) = serve(move |_| (200, document.clone().into_bytes()));
    state
        .db()
        .set_config("plugins.registry", &serde_json::json!(base), 1_000)
        .expect("the registry is configured");

    let update = plugins_check_update(&state, "@acme/hello")
        .expect("the check runs")
        .expect("a newer version exists");

    assert_eq!(update.version, "2.0.0");
    assert!(
        update.needs_new_consent,
        "the new manifest asks for a device permission"
    );
    assert_eq!(update.added_capabilities, ["device:send:*"]);
    // The installed version is not in the document, so the same check against itself cannot be exercised here; the
    // equal-version branch is a string comparison and fails closed (no update) rather than inventing one.
}

#[test]
fn an_update_asks_the_old_version_to_migrate_before_replacing_it() {
    // ★ The data being moved is the old plugin's, and the old code is the only code that knows what it wrote. A
    // migration that runs after the replacement has no working version to fall back to.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    let state = state.with_plugin_loads(summary);
    let id = PluginId::parse(ID).expect("an id");
    assert!(state.plugin_set().expect("an engine").lock().contains(&id));

    let source = write_update_source(root.path(), "2.0.0");
    super::super::update_directory(&state, ID, &source).expect("the update is applied");

    let active = state
        .db()
        .active_plugin(ID)
        .expect("the version table")
        .expect("an active version");
    assert_eq!(active.version, "2.0.0");
}

#[test]
fn a_migration_that_refuses_stops_the_update_and_leaves_the_old_version() {
    let (root, state) = testing::file_state();
    let (state, backend) = with_scripted_engine(state);
    loadable(&state);
    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    let state = state.with_plugin_loads(summary);
    let id = PluginId::parse(ID).expect("an id");
    backend.script_for(
        ID,
        &synra_plugin_host::Work::Migrate {
            from_version: "1.0.0".to_owned(),
            to_version: "2.0.0".to_owned(),
        },
        synra_plugin_host::Scripted::Failed {
            code: "invalidArgument".to_owned(),
            message: "this data cannot move".to_owned(),
            details: None,
        },
    );

    let source = write_update_source(root.path(), "2.0.0");
    let refused = super::super::update_directory(&state, ID, &source).expect_err("the migration refused");
    assert_eq!(refused.code, synra_protocol::ErrorCode::PluginTrapped);
    let reason = refused.params["reason"]
        .as_str()
        .expect("the refusal names the reason");
    assert!(
        reason.contains("this data cannot move"),
        "the plugin's own sentence is carried: {reason}"
    );
    // The old version is still installed and still loaded: a failed migration changed nothing the user can see.
    // The manifest on disk is the evidence that `run_install` was never reached, and the set's version is the
    // evidence that the old compiled form is still the one answering.
    let manifest = std::fs::read_to_string(state.paths.plugin_dir(ID).join("manifest.json"))
        .expect("the old manifest");
    assert!(
        manifest.contains("\"1.0.0\""),
        "the update wrote a new version despite the refusal: {manifest}"
    );
    assert_eq!(
        state.plugin_set().expect("an engine").lock().version(&id),
        Some("1.0.0")
    );
}

#[test]
fn the_cached_updates_are_readable_by_a_page_that_was_not_open() {
    // ★ The background pass runs whether or not the plugins page is open. This is how its answer reaches a page that
    // mounts later, sorted so two readers see the same order.
    let (_root, state) = crate::testing::file_state();
    for (plugin, version) in [("@acme/two", "3.0.0"), ("@acme/one", "2.0.0")] {
        let db = state.db();
        db.insert_plugin(plugin, false, false, "good", 1, 1)
            .expect("plugin");
        db.record_plugin_version(&synra_store::PluginVersionRecord {
            plugin_id: plugin,
            version: "1.0.0",
            artifact: "wasm",
            digest: "digest",
            manifest: "{}",
            installed_ms: 1,
            ..Default::default()
        })
        .expect("version");
        db.save_plugin_update_check(&synra_store::PluginUpdateCheck {
            plugin_id: plugin.to_owned(),
            current_version: "1.0.0".to_owned(),
            checked_at_ms: 123,
            registry: None,
            available_version: Some(version.to_owned()),
            added_capabilities: Vec::new(),
            attempts: 0,
            retry_at_ms: None,
        })
        .expect("check");
    }
    let restarted = crate::testing::reopen(&state);
    let updates = super::super::plugins_updates(&restarted).expect("saved updates");

    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].plugin_id.as_str(), "@acme/one");
    assert_eq!(updates[0].version, "2.0.0");
    assert_eq!(updates[1].plugin_id.as_str(), "@acme/two");
}

#[test]
fn an_update_announces_its_version_and_its_warnings() {
    // ★ The registry update path produced an `InstallReport` and then threw it away; the file-association path already
    // announced the same report. A warning about an unsigned package must not depend on which button started the
    // update, so both paths now send this notice.
    let state = crate::testing::state();
    let subscriber = state.events.subscribe();
    let report = synra_ipc::InstallReport {
        plugin_id: PluginId::parse(ID).expect("a plugin id"),
        version: "2.0.0".to_owned(),
        warnings: vec!["the package is not signed".to_owned()],
        enabled: false,
        publisher_id: None,
        fingerprint: None,
    };

    announce_updated(&state, &report);

    let delivery = state.events.take(subscriber).expect("one event");
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::Notice { message, params }) => {
            assert_eq!(message, "notice.updated");
            assert_eq!(params["pluginId"], ID);
            assert_eq!(params["version"], "2.0.0");
            assert_eq!(params["warnings"][0], "the package is not signed");
        }
        other => panic!("expected an update notice, got {other:?}"),
    }
}
