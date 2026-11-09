use super::{
    AUDIT_RETENTION_KEY, announce_updated, discover_plugins, install_directory,
    load_installed_plugins, plugins_audit, plugins_audit_clear, plugins_catalog_search,
    plugins_check_update, plugins_consent_respond, plugins_install, plugins_list, plugins_preview,
    plugins_publisher_decide, plugins_publishers, prune_audit, record_card,
};
use crate::testing;
use synra_ipc::{CatalogQuery, InstallSourceDto};
use synra_protocol::PluginId;

const ID: &str = "@synra-plugin/chat";

#[test]
fn a_faulted_plugin_is_not_drawn_as_active() {
    // ★ The set holds the bytes and the lifecycle holds the fault. "Loaded" was the whole question before, so a
    // plugin the health ladder had stopped calling still got the green tag of one that was running.
    let row = synra_store::PluginRow {
        plugin_id: ID.to_owned(),
        enabled: true,
        trusted: true,
        health: "faulted".to_owned(),
        installed_at_ms: 0,
        updated_at_ms: 0,
    };
    assert_eq!(super::life_state(&row, true, true, true, false), "faulted");
    assert_eq!(super::life_state(&row, true, false, true, false), "active");
}

/// Every word `life_state` can return, and nothing else.
///
/// ★ The interface has to name each of these: `messages.plugins.states` is keyed by the word, and a word without
///   an entry falls through to itself — a Chinese card reading `awaiting_consent`. The front end pins the same six
///   in `messages-plugins.test.ts`'s `it.each`, but a list that only names six can only prove those six exist.
///   Nothing on this side said which words there are, so a seventh `return` above used to reach the screen as an
///   English identifier with no test going red.
///
///   This test is the inventory. Its six must equal the six `return`s — that is the one check that can fail when
///   somebody adds a state and forgets the front end.
#[test]
fn life_state_returns_exactly_these_words() {
    let plugin = synra_store::PluginRow {
        plugin_id: ID.to_owned(),
        enabled: true,
        trusted: true,
        health: "ok".to_owned(),
        installed_at_ms: 0,
        updated_at_ms: 0,
    };
    let off = synra_store::PluginRow {
        enabled: false,
        ..plugin.clone()
    };
    let untrusted = synra_store::PluginRow {
        trusted: false,
        ..plugin.clone()
    };

    // Arguments after the row: loaded, faulted, manifest readable, load refused.
    assert_eq!(
        super::life_state(&off, true, false, true, false),
        "disabled"
    );
    assert_eq!(
        super::life_state(&untrusted, true, false, true, false),
        "awaiting_consent"
    );
    assert_eq!(
        super::life_state(&plugin, true, false, false, false),
        "unreadable"
    );
    assert_eq!(
        super::life_state(&plugin, true, true, true, false),
        "faulted"
    );
    assert_eq!(
        super::life_state(&plugin, true, false, true, false),
        "active"
    );
    assert_eq!(
        super::life_state(&plugin, false, false, true, false),
        "unavailable"
    );
}

/// A state with a scripted engine, because installing needs one and the fixture builds a shell without.
///
/// The backend answers from a table rather than compiling Wasm: what is being tested is the shell's command — the
/// source it accepts, the report it returns and the row that lands — and the pipeline behind it has its own tests.
fn with_engine(state: crate::state::AppState) -> crate::state::AppState {
    with_scripted_engine(state).0
}

/// The same engine, with the backend kept so a test can script one entry point.
fn with_scripted_engine(
    state: crate::state::AppState,
) -> (crate::state::AppState, synra_plugin_host::ScriptedBackend) {
    let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::with_platform(
        std::sync::Arc::clone(&state.db),
        std::sync::Arc::clone(&state.platform),
    ));
    let scripted = synra_plugin_host::ScriptedBackend::new();
    let backend = synra_plugin_host::Backend::new(std::sync::Arc::new(scripted.clone()));
    (
        state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(
            synra_core::dispatch::PluginSet::new(backend, host),
        ))),
        scripted,
    )
}

/// A package directory a local install will accept: a manifest and a component.
fn package(root: &std::path::Path) -> std::path::PathBuf {
    let directory = root.join("package");
    std::fs::create_dir_all(&directory).expect("a package directory");
    let manifest = format!(
        r#"{{"id": "@acme/hello", "version": "1.0.0", "title": "Hello",
                "publisher": {{"id": "@acme"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    directory
}

/// A `.tgz` holding these entries, written in memory.
fn tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *bytes)
            .expect("an entry");
    }
    builder
        .into_inner()
        .expect("the encoder")
        .finish()
        .expect("a gzip tail")
}

#[test]
fn a_registry_install_downloads_and_opens_the_tarball() {
    // ★ The whole first path: packument → tarball → extraction → the existing install pipeline. The server is a local
    // listener, so the test proves the client rather than a network.
    use std::io::{BufRead, BufReader, Write as _};

    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());
    let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
    let manifest_json: serde_json::Value =
        serde_json::from_slice(&manifest).expect("a manifest document");

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let base = format!("http://{address}");
    let archive = tgz(&[
        ("package/manifest.json", manifest.as_slice()),
        (
            "package/plugin.wasm",
            synra_testkit::component::inert_component()
                .expect("a valid component")
                .as_slice(),
        ),
    ]);
    // ★ The document declares the digest of the bytes it points at, and the install checks them: a fake registry with
    // no integrity is now refused before anything is unpacked, which is the property the test below would hide if
    // this one omitted it.
    let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();
    let document = serde_json::json!({
        "name": "@acme/hello",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/hello",
                "version": "1.0.0",
                "dist": { "tarball": format!("{base}/pkg.tgz"), "integrity": integrity },
                "synra": manifest_json
            }
        }
    })
    .to_string();
    std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok(mut stream) = listener.accept().map(|(stream, _)| stream) else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let body: &[u8] = if first.contains("pkg.tgz") {
                &archive
            } else {
                document.as_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
    });
    state
        .db()
        .set_config("plugins.registry", &serde_json::json!(base), 1_000)
        .expect("the registry is configured");

    // ★ The preview is read before anything is downloaded, from the packument's inline manifest. It shows what the
    // package asks for; the install that follows still verifies the bytes and records the plugin switched off.
    let preview = plugins_preview(&state, "@acme/hello", None).expect("a preview");
    assert_eq!(preview.plugin_id.as_str(), "@acme/hello");
    assert_eq!(preview.version, "1.0.0");
    assert_eq!(preview.capabilities, ["log:*"]);
    assert!(preview.headless && !preview.has_ui);

    let report = plugins_install(
        &state,
        &InstallSourceDto::Registry {
            plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
            version: None,
        },
    )
    .expect("the registry package installs");

    assert_eq!(report.plugin_id.as_str(), "@acme/hello");
    assert_eq!(report.version, "1.0.0");
    assert!(
        state
            .db()
            .plugins()
            .expect("rows")
            .iter()
            .any(|row| row.plugin_id == "@acme/hello")
    );
    // The cache directory the tarball was opened in is gone, not left for the next run to find.
    let cache = state.paths.cache_dir().join("registry");
    assert!(
        !cache
            .read_dir()
            .map(|entries| entries.count() > 0)
            .unwrap_or(false),
        "the extracted package was left in the cache"
    );
}

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

#[test]
fn recording_consent_tells_the_interface() {
    // ★ The consent command published an event since it was wired, and nothing asserted it. The enable path has a
    // test; this one is the other half — a decision recorded from the interface has to reach the interface, or the
    // card it returns and the list another window draws disagree about whether the user approved anything.
    let (_root, state) = installed();
    let subscriber = state.events.subscribe();

    plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
            assert_eq!(card.plugin_id.as_str(), ID);
            assert!(
                !card.granted.is_empty(),
                "the card carries the decision that was just recorded"
            );
        }
        other => panic!("expected a plugin card, got {other:?}"),
    }
}
#[test]
fn enabling_a_plugin_tells_the_interface() {
    // ★ The producer the event dispatcher never had. A switch that flipped in the store and nowhere else is what
    // "the shell never emits" looked like from the window: the page showed whatever it had read when it opened.
    let (_root, state) = installed();
    let subscriber = state.events.subscribe();

    super::plugins_enable(&state, ID).expect("the plugin is enabled");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(
        delivery.first_seq(),
        1,
        "the bus numbers it, not this command"
    );
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
            assert_eq!(card.plugin_id.as_str(), ID);
            assert!(card.enabled, "the card says what the row now says");
        }
        other => panic!("expected a plugin card, got {other:?}"),
    }
}

#[test]
fn a_registry_install_without_a_configured_registry_is_refused() {
    // ★ Refused by the missing configuration, not by a network failure: there is nowhere to dial, and "could not reach
    // the registry" would send a user to check a connection that was never used.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);

    let refused = plugins_install(
        &state,
        &InstallSourceDto::Registry {
            plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
            version: None,
        },
    )
    .expect_err("no registry is configured");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
}

#[test]
fn a_catalog_search_lists_what_the_registry_index_returns() {
    // ★ Results are filtered to plugin ids: an npm index scoped by keyword can still return a package whose name is
    // not one this device could install, and showing it would offer a button that cannot work.
    use std::io::{BufRead, BufReader, Write as _};

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

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let base = format!("http://{address}");
    let body = r#"{"objects":[
            {"package":{"name":"@acme/zeta","version":"2.0.0","publisher":{"username":"acme"}}},
            {"package":{"name":"@acme/hello","version":"1.0.0","description":"Hello","publisher":{"username":"acme"}}},
            {"package":{"name":"not a plugin","version":"1.0.0"}}
        ]}"#;
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
        let mut first = String::new();
        let _ = reader.read_line(&mut first);
        // ★ The page is the parameter npm spells `from`; a client that forgot it would ask for page one forever.
        assert!(first.contains("from=0"), "the offset was not sent: {first}");
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
    });
    state
        .db()
        .set_config("plugins.registry", &serde_json::json!(base), 1_000)
        .expect("the registry is configured");

    let found = plugins_catalog_search(
        &state,
        &CatalogQuery {
            query: "hello".to_owned(),
            registry: None,
            limit: Some(10),
            offset: Some(0),
            sort: Some(synra_ipc::dto::CatalogSort::Name),
        },
    )
    .expect("a search");

    assert_eq!(
        found.len(),
        2,
        "a name that is not a plugin id is filtered out"
    );
    assert_eq!(
        found[0].plugin_id.as_str(),
        "@acme/hello",
        "name sort puts hello before zeta"
    );
    assert_eq!(found[1].plugin_id.as_str(), "@acme/zeta");
    assert_eq!(found[0].version, "1.0.0");
    assert_eq!(found[0].publisher, "acme");
    assert!(found[0].installed, "the fixture is installed");
}

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

/// A tiny HTTP server for the registry tests: answers each request from `handler`, and counts what it served.
fn serve(
    handler: impl Fn(&str) -> (u16, Vec<u8>) + Send + 'static,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{BufRead, BufReader, Write as _};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let (status, body) = handler(&first);
            let reason = if status == 200 { "OK" } else { "Not Found" };
            let head = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (format!("http://{address}"), served)
}

#[test]
fn a_mirror_that_fails_is_followed_by_the_next_one() {
    // ★ The first mirror serves the packument and then refuses its own tarball; the second serves the same path. The
    // install must succeed, and the second mirror must have been asked — a client that retried the first one only
    // would pass a test that only counted the install.
    use std::sync::{Arc, Mutex};

    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());
    let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
    let archive = tgz(&[
        ("package/manifest.json", manifest.as_slice()),
        (
            "package/plugin.wasm",
            synra_testkit::component::inert_component()
                .expect("a valid component")
                .as_slice(),
        ),
    ]);
    let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();

    let (second, second_count) = serve(move |line: &str| {
        if line.contains("pkg.tgz") {
            (200, archive.clone())
        } else {
            (404, Vec::new())
        }
    });
    let document = Arc::new(Mutex::new(String::new()));
    let served_document = Arc::clone(&document);
    let (first, first_count) = serve(move |line: &str| {
        if line.contains("%2F") {
            (
                200,
                served_document
                    .lock()
                    .expect("the document")
                    .clone()
                    .into_bytes(),
            )
        } else {
            (404, Vec::new())
        }
    });
    *document.lock().expect("the document") = serde_json::json!({
        "name": "@acme/hello",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/hello",
                "version": "1.0.0",
                // On the first mirror, whose `/pkg.tgz` is a 404; the second mirror serves the same path.
                "dist": { "tarball": format!("{first}/pkg.tgz"), "integrity": integrity }
            }
        }
    })
    .to_string();

    state
        .db()
        .set_config(
            "plugins.registry",
            &serde_json::json!([first, second]),
            1_000,
        )
        .expect("the mirrors are configured");

    let report = plugins_install(
        &state,
        &InstallSourceDto::Registry {
            plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
            version: None,
        },
    )
    .expect("the second mirror completes the install");

    assert_eq!(report.version, "1.0.0");
    assert!(
        first_count.load(std::sync::atomic::Ordering::Relaxed) >= 1,
        "the first mirror was not asked"
    );
    assert!(
        second_count.load(std::sync::atomic::Ordering::Relaxed) >= 1,
        "the second mirror was never asked"
    );
}

/// A manifest declaring the two capabilities the consent tests decide about.
///
/// The shape is the one `synra-plugin-api`'s own fixture uses, because a hand-written manifest that drifted from it
/// would test the parser rather than this command.
const MANIFEST: &str = r#"{
        "id": "@synra-plugin/chat",
        "version": "1.0.0",
        "title": "Chat",
        "publisher": { "id": "synra-official" },
        "sdk": { "min": "1.0.0" },
        "wit": "synra:plugin@1.0.0",
        "runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } },
        "capabilities": [{ "cap": "log:*" }, { "cap": "device:list" }]
    }"#;

/// A state with the plugin installed on disk and in the database, awaiting consent.
fn installed() -> (testing::Root, crate::state::AppState) {
    let (root, state) = testing::file_state();
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), MANIFEST).expect("a manifest");
    state
        .db()
        .insert_plugin(
            ID,
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    (root, state)
}

fn grants(state: &crate::state::AppState) -> Vec<(String, bool)> {
    state
        .db()
        .plugin_grants(ID)
        .expect("the decisions")
        .into_iter()
        .map(|row| (row.capability, row.granted))
        .collect()
}

#[test]
fn a_static_index_installs_the_tarball_it_names() {
    // ★ A bucket has no packument endpoint: one JSON document per package names the versions and their tarballs, and
    // the install path is the same one an npm registry feeds.
    use std::sync::{Arc, Mutex};

    let (root, state) = testing::file_state();
    let state = with_engine(state);
    let source = package(root.path());
    let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
    let manifest_json: serde_json::Value =
        serde_json::from_slice(&manifest).expect("a manifest document");
    let archive = tgz(&[
        ("package/manifest.json", manifest.as_slice()),
        (
            "package/plugin.wasm",
            synra_testkit::component::inert_component()
                .expect("a valid component")
                .as_slice(),
        ),
    ]);
    let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();

    let document = Arc::new(Mutex::new(String::new()));
    let served = Arc::clone(&document);
    let (base, _served) = serve(move |line: &str| {
        if line.contains("hello.json") {
            (
                200,
                served.lock().expect("the document").clone().into_bytes(),
            )
        } else if line.contains("pkg.tgz") {
            (200, archive.clone())
        } else {
            (404, Vec::new())
        }
    });
    *document.lock().expect("the document") = serde_json::json!({
        "name": "@acme/hello",
        "latest": "1.0.0",
        "publishers": { "@acme": "AB12 CD34 EF56 7890" },
        "versions": [{
            "version": "1.0.0",
            // Relative to the index's own URL: the client resolves it, so the author need not know the bucket.
            "tarball": "pkg.tgz",
            // And the digest of the bytes it names; without one the install refuses before unpacking.
            "integrity": integrity,
            "manifest": manifest_json
        }]
    })
    .to_string();
    state
        .db()
        .set_config(
            "plugins.registry",
            &serde_json::json!(format!("static:{base}")),
            1_000,
        )
        .expect("the static index is configured");

    // ★ The directory's fingerprint is shown before the tarball is downloaded; the signature is still checked on the
    // bytes after it arrives.
    let preview = plugins_preview(&state, "@acme/hello", None).expect("a preview");
    assert_eq!(
        preview.publisher_key.as_deref(),
        Some("AB12 CD34 EF56 7890")
    );

    let report = plugins_install(
        &state,
        &InstallSourceDto::Registry {
            plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
            version: None,
        },
    )
    .expect("the static index installs");

    assert_eq!(report.plugin_id.as_str(), "@acme/hello");
    assert_eq!(report.version, "1.0.0");
}

#[test]
fn a_static_index_is_cached_by_its_validator() {
    // ★ The second request has to be conditional and the second answer has to come from the cache: a host that sends a
    // different body when asked without `If-None-Match` is how this test tells the two paths apart.
    use std::io::{BufRead, BufReader, Write as _};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let base = format!("http://{address}");
    let first_body = |base: &str, latest: &str| {
        serde_json::json!({
            "name": "@acme/hello",
            "latest": latest,
            "versions": [{
                "version": latest,
                "tarball": format!("{base}/hello-{latest}.tgz"),
                "manifest": {
                    "id": "@acme/hello",
                    "version": latest,
                    "title": "Hello",
                    "publisher": { "id": "@acme" },
                    "sdk": { "min": "0.1.0" },
                    "wit": synra_plugin_host::host_world(),
                    "capabilities": [{ "cap": "log:*" }],
                    "runtime": { "headless": { "artifact": "plugin.wasm", "stateful": false } }
                }
            }]
        })
        .to_string()
    };
    let requests = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&requests);
    let base_in_thread = base.clone();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let _ = counted.fetch_add(1, Ordering::Relaxed);
            if headers
                .to_ascii_lowercase()
                .contains("if-none-match: \"v1\"")
            {
                let _ = stream.write_all(
                    b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            } else {
                // A document that is deliberately different, so a caller that skipped the cache would see it.
                let body = first_body(&base_in_thread, "3.0.0");
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\netag: \"v1\"\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        }
    });
    state
        .db()
        .set_config(
            "plugins.registry",
            &serde_json::json!(format!("static:{base}")),
            1_000,
        )
        .expect("the static index is configured");

    let first = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");
    let second = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");

    assert_eq!(first.version, "3.0.0");
    assert_eq!(
        second.version, "3.0.0",
        "the 304 answered from the cache, not from a second body"
    );
    assert_eq!(requests.load(Ordering::Relaxed), 2);
}

#[test]
fn an_npm_packument_is_cached_by_its_etag() {
    // ★ The validators are the registry's whether it speaks npm or serves a bucket. A check-update that re-reads the
    // whole packument for a document that has not changed is a round trip this side chose not to save.
    use std::io::{BufRead, BufReader, Write as _};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let base = format!("http://{address}");
    let body = serde_json::json!({
        "name": "@acme/hello",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "2.0.0": {
                "name": "@acme/hello",
                "version": "2.0.0",
                "dist": { "tarball": format!("{base}/pkg.tgz") },
                "synra": {
                    "id": "@acme/hello",
                    "version": "2.0.0",
                    "title": "Hello",
                    "publisher": { "id": "@acme" },
                    "sdk": { "min": "0.1.0" },
                    "wit": synra_plugin_host::host_world(),
                    "capabilities": [{ "cap": "log:*" }],
                    "runtime": { "headless": { "artifact": "plugin.wasm", "stateful": false } }
                }
            }
        }
    })
    .to_string();
    let requests = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&requests);
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let _ = counted.fetch_add(1, Ordering::Relaxed);
            if headers
                .to_ascii_lowercase()
                .contains("if-none-match: \"p1\"")
            {
                let _ = stream.write_all(
                    b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            } else {
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\netag: \"p1\"\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        }
    });
    state
        .db()
        .set_config("plugins.registry", &serde_json::json!(base), 1_000)
        .expect("the registry is configured");

    let first = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");
    let second = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");

    assert_eq!(first.version, "2.0.0");
    assert_eq!(second.version, "2.0.0", "the 304 answered from the cache");
    assert_eq!(requests.load(Ordering::Relaxed), 2);
}

#[test]
fn an_expired_static_index_is_read_again() {
    // ★ `max-age=0` says the document is good for no time at all. The second call must not send a validator and must
    // read the new body — a cache that ignored the lifetime would keep an old version until the process ended.
    use std::io::{BufRead, BufReader, Write as _};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let base = format!("http://{address}");
    let requests = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&requests);
    let base_in_thread = base.clone();
    let body = move |latest: &str| {
        serde_json::json!({
            "name": "@acme/hello",
            "latest": latest,
            "versions": [{
                "version": latest,
                "tarball": format!("{base_in_thread}/hello-{latest}.tgz"),
                "manifest": {
                    "id": "@acme/hello",
                    "version": latest,
                    "title": "Hello",
                    "publisher": { "id": "@acme" },
                    "sdk": { "min": "0.1.0" },
                    "wit": synra_plugin_host::host_world(),
                    "capabilities": [{ "cap": "log:*" }],
                    "runtime": { "headless": { "artifact": "plugin.wasm", "stateful": false } }
                }
            }]
        })
        .to_string()
    };
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let seen = counted.fetch_add(1, Ordering::Relaxed);
            if headers.to_ascii_lowercase().contains("if-none-match") {
                let _ = stream.write_all(
                    b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            } else {
                let document = if seen == 0 {
                    body("3.0.0")
                } else {
                    body("4.0.0")
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\ncache-control: max-age=0\r\netag: \"v1\"\r\nconnection: close\r\n\r\n",
                    document.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(document.as_bytes());
            }
        }
    });
    state
        .db()
        .set_config(
            "plugins.registry",
            &serde_json::json!(format!("static:{base}")),
            1_000,
        )
        .expect("the static index is configured");

    let first = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");
    let second = plugins_check_update(&state, "@acme/hello")
        .expect("a check")
        .expect("an update");

    assert_eq!(first.version, "3.0.0");
    assert_eq!(
        second.version, "4.0.0",
        "the expired document was read again"
    );
    assert_eq!(requests.load(Ordering::Relaxed), 2);
}

#[test]
fn a_static_index_has_no_search_to_offer() {
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    state
        .db()
        .set_config(
            "plugins.registry",
            &serde_json::json!("static:https://static.example.com"),
            1_000,
        )
        .expect("the static index is configured");

    let refused = plugins_catalog_search(
        &state,
        &CatalogQuery {
            query: "hello".to_owned(),
            registry: None,
            limit: None,
            offset: None,
            sort: None,
        },
    )
    .expect_err("there is no index to search");

    assert_eq!(
        refused.code,
        synra_protocol::ErrorCode::Unsupported,
        "{refused:?}"
    );
}

#[test]
fn a_card_says_whether_the_plugin_has_a_page() {
    // ★ `has_ui` is not `hybrid`: a UI-only plugin has a page and no headless half, and `plugin_ui_open` can open
    // it. The card needs both facts separately, or the one shape with a page but no Wasm would have no way in.
    let (_root, state) = installed();

    let headless_only = plugins_list(&state).expect("the list").remove(0);
    assert!(
        !headless_only.has_ui,
        "a headless-only manifest has no page"
    );
    assert!(!headless_only.hybrid);

    // The same plugin with a UI half written over its manifest. The entry is what `plugin_ui_open` resolves, so a
    // card that reported `has_ui` without one would draw a button that cannot work.
    let manifest = MANIFEST.replace(
        r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } }"#,
        r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false },
                "ui": { "artifact": "dist/ui", "entry": "index.html" } }"#,
    );
    std::fs::write(state.paths.plugin_dir(ID).join("manifest.json"), manifest)
        .expect("a manifest with a page");

    let with_page = plugins_list(&state).expect("the list").remove(0);
    assert!(with_page.has_ui, "the page entry is not reported");
    assert!(with_page.hybrid, "this manifest has both halves");
}

/// A recorded row, filled in with the fields the mapping reads.
fn row(
    plugin: &str,
    capability: &str,
    outcome: &str,
    at_ms: i64,
) -> synra_store::records::AuditRecord {
    synra_store::records::AuditRecord {
        plugin_id: plugin.to_owned(),
        capability: capability.to_owned(),
        outcome: outcome.to_owned(),
        at_ms,
        elapsed_ms: Some(7),
        trace_id: None,
    }
}

#[test]
fn a_recorded_call_is_mapped_from_its_row() {
    // ★ What this command is responsible for. The table's own read is tested where it lives; what is left here is the
    // mapping, and a wrong mapping looks right — an allowed call reported with a reason, or a refusal reported as
    // allowed.
    let allowed = record_card(&row(ID, "log:info", "allowed", 1_700_000_000_000));
    assert_eq!(allowed.capability, "log:info");
    assert!(allowed.allowed);
    assert_eq!(allowed.elapsed_ms, Some(7));
    assert_eq!(
        allowed.reason, None,
        "an allowed call has no reason to give"
    );

    let denied = record_card(&row(ID, "device:list", "denied", 1_700_000_000_001));
    assert!(!denied.allowed);
    // The outcome's stable name, which the interface translates — not a sentence written here.
    assert_eq!(denied.reason.as_deref(), Some("denied"));
    // A row recorded without an action behind it stays without one — `None` here is a fact about the call, not a
    // mapping that failed.
    assert_eq!(denied.trace_id, None);

    // ★ And a row with one gives the DTO one. The column is new; this is the half that proves the mapping reads it
    // rather than still answering `None` from an older shape.
    let mut traced = row(ID, "device:list", "allowed", 1_700_000_000_003);
    traced.trace_id = Some("01a150c8-c332-768d-9dd1-981502aa1854".to_owned());
    assert_eq!(
        record_card(&traced).trace_id.as_deref(),
        Some("01a150c8-c332-768d-9dd1-981502aa1854")
    );

    // ★ "Permitted by the gate, failed in the operation" is not a refusal — `AuditOutcome::was_allowed` says so, and a
    // mapping that compared the name against `"allowed"` would draw this call as blocked.
    let failed = record_card(&row(ID, "network:fetch", "failed", 1_700_000_000_002));
    assert!(failed.allowed, "the gate permitted this call");
    assert_eq!(failed.reason, None);

    // A name from a newer build is reported rather than guessed at: presenting it as allowed could hide a refusal.
    let unknown = record_card(&row(
        ID,
        "device:list",
        "from-a-newer-build",
        1_700_000_000_003,
    ));
    assert!(!unknown.allowed);
    assert_eq!(unknown.reason.as_deref(), Some("from-a-newer-build"));
}

#[test]
fn the_audit_reads_the_trail_that_outlives_the_engine() {
    // ★ The fixture has no engine, and that must not matter: the table is written by whichever host ran the call, so a
    // build that cannot run plugins can still answer what this database recorded. The old `Unsupported` answer was
    // true only while the in-memory ring was the whole trail.
    let state = crate::testing::state();
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", 5_000))
        .expect("recorded");

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].capability, "device:list");
    assert_eq!(rows[0].at_ms, 5_000);
}

#[test]
fn the_audit_read_can_skip_the_rows_a_page_already_showed() {
    // ★ "Show more" is wired through this parameter. A command that ignored it would repeat the first page, which is
    // the one failure a list that appends rather than replaces would show as duplicated rows rather than as nothing.
    let state = crate::testing::state();
    for at in [1_000, 2_000, 3_000] {
        state
            .db()
            .record_audit(&row(ID, "device:list", "allowed", at))
            .expect("recorded");
    }

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 1, 1).expect("read");

    assert_eq!(rows.len(), 1, "one row asked for, one row answered");
    assert_eq!(
        rows[0].at_ms, 2_000,
        "and it is the second-newest, not the newest again"
    );
}

#[test]
fn clearing_forgets_the_durable_rows_and_leaves_another_plugin() {
    // The same rule as the ring, now on the copy that survives a restart.
    let state = crate::testing::state();
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", 5_000))
        .expect("recorded");
    state
        .db()
        .record_audit(&row("@acme/other", "log:info", "allowed", 6_000))
        .expect("recorded");

    plugins_audit_clear(&state, &PluginId::parse(ID).expect("a plugin id")).expect("cleared");

    assert_eq!(state.db().audit_count().expect("counted"), 1);
    assert!(
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0)
            .expect("read")
            .is_empty()
    );
}

#[test]
fn old_calls_are_pruned_and_recent_ones_kept() {
    // ★ The table is written on every gated call and nothing in this shell swept it, so a long-running install would
    // grow it for ever. With no configured window the design's seven days is the rule.
    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 10 * day))
        .expect("recorded");
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 2 * day))
        .expect("recorded");

    assert_eq!(prune_audit(&state, now), 1);

    let rows =
        plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at_ms, now - 2 * day, "the recent call stayed");
}

#[test]
fn the_retention_window_comes_from_the_configuration() {
    // ★ The setting existed only as a constant. Reading it is what turns "the design names a key" into the key doing
    // something: a one-day window has to remove the two-day-old row that the default seven kept in the test above.
    use synra_store::ConfigChange;

    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 10 * day))
        .expect("recorded");
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now - 2 * day))
        .expect("recorded");
    state
        .db()
        .apply_config(
            &[ConfigChange {
                key: AUDIT_RETENTION_KEY.to_owned(),
                value: Some(serde_json::json!(1)),
            }],
            now,
        )
        .expect("the setting is written");

    assert_eq!(
        prune_audit(&state, now),
        2,
        "a one-day window removes both calls"
    );
}

#[test]
fn a_retention_outside_the_sane_range_is_clamped_rather_than_trusted() {
    // ★ The file can be edited by hand, and `0` would delete today's calls — including the refusal the user just
    // looked at. The clamp is what makes the setting a window rather than a way to empty the table from a typo.
    use synra_store::ConfigChange;

    let state = crate::testing::state();
    let day = 24 * 60 * 60 * 1_000;
    let now = 30 * day;
    state
        .db()
        .record_audit(&row(ID, "device:list", "allowed", now))
        .expect("recorded");
    state
        .db()
        .apply_config(
            &[ConfigChange {
                key: AUDIT_RETENTION_KEY.to_owned(),
                value: Some(serde_json::json!(0)),
            }],
            now,
        )
        .expect("the setting is written");

    assert_eq!(
        prune_audit(&state, now),
        0,
        "zero becomes the one-day minimum, and today's call stays"
    );
}

#[test]
fn listing_plugins_sets_the_installed_and_active_gauges() {
    // ★ Set where the truth is read rather than incremented on every write: a write that failed half-way leaves no
    // counter to correct. The fixture installs one enabled plugin; a second, disabled row makes the two numbers
    // differ — an assertion that only checked one gauge would pass even if both were fed the same value.
    use synra_observe::metrics::Value;

    let (_root, state) = installed();
    state
        .db()
        .insert_plugin(
            "@acme/other",
            false,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a second row");

    let _ = plugins_list(&state).expect("the list");

    let snapshot = state.metrics.snapshot();
    assert_eq!(snapshot.get("plugins_installed"), Some(&Value::Gauge(2)));
    assert_eq!(
        snapshot.get("plugins_active"),
        Some(&Value::Gauge(1)),
        "only the enabled one is active"
    );
}

#[test]
fn listing_plugins_reports_the_bytes_each_sandbox_holds() {
    // ★ The storage group's per-plugin gauge: the design names it, and nothing measured it. Nested files are in the
    // fixture because a top-level-only walk would pass a test that only wrote one file at the root.
    use synra_observe::metrics::Value;

    let (_root, state) = installed();
    let sandbox = state.paths.plugin_sandbox_dir(ID);
    std::fs::create_dir_all(sandbox.join("nested")).expect("the sandbox");
    std::fs::write(sandbox.join("cache.bin"), [1u8; 10]).expect("a file");
    std::fs::write(sandbox.join("nested").join("more.bin"), [2u8; 5]).expect("a nested file");

    let _ = plugins_list(&state).expect("the list");

    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("plugin_data_bytes{plugin=@synra-plugin/chat}"),
        Some(&Value::Gauge(15))
    );
}

#[test]
fn granting_every_capability_makes_the_plugin_trusted_and_records_each_decision() {
    // ★ The act the whole table was built for, and the one nothing could perform: `plugin_grants` had a schema, an
    // index, a row type, a reader and **no writer at all**, so the loader took the manifest's own declaration as the
    // grant and a plugin received exactly what it asked for.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent");

    assert_ne!(
        card.state, "awaiting_consent",
        "the plugin is trusted now: {card:?}"
    );
    assert_eq!(
        card.granted,
        vec!["device:list".to_owned(), "log:*".to_owned()]
    );
    assert!(card.denied.is_empty());
    assert_eq!(
        grants(&state),
        vec![("device:list".to_owned(), true), ("log:*".to_owned(), true)],
        "both decisions are on disk"
    );
}

#[test]
fn declining_one_capability_records_the_denial_and_still_trusts() {
    // ★ Two claims in one test, and the second is the one this host used to get wrong.
    //
    // A declined capability is recorded as a **denial**, not omitted: a plugin whose manifest declares two
    // capabilities and whose table holds one row is one the interface cannot describe, so the user is shown a
    // capability they never answered about.
    //
    // And the plugin is **trusted anyway**: it may run with what it was allowed and be refused at the gate for the
    // rest. This host used to require every capability, the daemon required any, and the loader took the rows as
    // the authority — so the same decision produced a plugin that could not run here and one that ran there.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

    assert_eq!(card.granted, vec!["log:*".to_owned()]);
    assert_eq!(card.denied, vec!["device:list".to_owned()]);
    assert_ne!(
        card.state, "awaiting_consent",
        "one allowed capability is approval"
    );
    assert_eq!(
        grants(&state),
        vec![
            ("device:list".to_owned(), false),
            ("log:*".to_owned(), true)
        ],
    );
}

#[test]
fn denying_every_capability_leaves_it_awaiting_consent() {
    // The other side of the rule: a decision that allows nothing is a refusal, and `should_run` is
    // `enabled && trusted` — so the row stays unable to run even though the user answered the prompt.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &[]).expect("consent");

    assert_eq!(card.state, "awaiting_consent");
    assert!(card.granted.is_empty());
    assert_eq!(card.denied.len(), 2);
}

#[test]
fn changing_a_decision_overwrites_it_rather_than_adding_a_second_answer() {
    // The table's primary key is `(plugin_id, capability)`, so a user who changes their mind must land on one row
    // per capability. Two rows for one capability would make "what did the user decide" unanswerable — and the
    // reader orders by capability, so the two would sit next to each other and look like two different facts.
    let (_root, state) = installed();

    plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("a first answer");
    assert_eq!(grants(&state).len(), 2);

    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("a changed answer");

    let decisions = grants(&state);
    assert_eq!(
        decisions.len(),
        2,
        "still one row per capability: {decisions:?}"
    );
    assert!(
        decisions.iter().all(|(_, granted)| *granted),
        "the second answer is the one that stands: {decisions:?}"
    );
}

#[test]
fn a_plugin_whose_manifest_cannot_be_read_is_refused_rather_than_consented_to_blind() {
    // ★ Treating an unreadable manifest as "declares nothing" would record an empty decision set, which is
    // indistinguishable from a user who declined everything — and would mark the plugin trusted while nothing had
    // been shown to anybody.
    let (root, state) = testing::file_state();
    state
        .db()
        .insert_plugin(
            ID,
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    drop(root);

    let refused = plugins_consent_respond(&state, ID, &[]);
    assert!(refused.is_err(), "a missing manifest is not a decision");
    assert!(grants(&state).is_empty(), "and nothing was recorded");
}

#[test]
fn the_decision_is_recorded_against_the_manifest_it_was_made_about() {
    // ★ Why `manifest_digest` is a column. A consent decision is about a *document*, and a plugin that ships an
    // update declaring more capabilities is a different document — so the rows that predate it are visibly older
    // than the manifest on disk. That is what makes silent capability growth detectable instead of invisible.
    let (_root, state) = installed();
    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent");
    let before = state.db().plugin_grants(ID).expect("decisions");
    let digest_before = before.first().expect("a row").manifest_digest.clone();
    assert_eq!(digest_before.len(), 64, "sha256 in hex: {digest_before}");

    // The same manifest decided about twice hashes the same, so an unchanged plugin does not look like a changed one.
    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent again");
    let after = state.db().plugin_grants(ID).expect("decisions");
    assert!(
        after.iter().all(|row| row.manifest_digest == digest_before),
        "an unchanged manifest must not look changed: {after:?}"
    );
}

#[test]
fn uninstalling_removes_the_row_and_keeps_the_data() {
    // ★ The default the design document asks for: a plugin the user may reinstall keeps its settings, and deleting
    // them unless asked would be the interface deciding what is replaceable.
    let (_root, state) = installed();
    let data = state.paths.plugin_sandbox_dir(ID);
    std::fs::create_dir_all(&data).expect("some plugin data");
    std::fs::write(data.join("cache.bin"), [7u8; 5]).expect("some bytes");

    let report = super::plugins_uninstall(&state, ID, false).expect("uninstalls");

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

    let report = super::plugins_uninstall(&state, ID, true).expect("uninstalls");

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
        super::plugins_uninstall(&state, "@acme/typo", false).expect_err("nothing is there");

    assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
}

#[test]
fn a_card_says_what_the_plugin_asks_for() {
    // ★ The field the consent prompt reads. `granted` and `denied` are both empty before the user answers, so a card
    // carrying only those could not say what to ask about — and the prompt would have to read the manifest itself,
    // which is the shell doing the kernel's job.
    let (_root, state) = installed();

    let cards = plugins_list(&state).expect("the list");

    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(
        card.requested,
        vec!["log:*".to_owned(), "device:list".to_owned()]
    );
    assert!(card.granted.is_empty(), "nothing has been decided yet");
    // ★ The path the removal dialog shows is the directory the removal would delete: both come from
    // `plugin_sandbox_dir`, and this assertion is what keeps the card from inventing a second spelling of it.
    assert_eq!(
        card.data_path,
        state.paths.plugin_sandbox_dir(ID).display().to_string(),
        "the card names the sandbox the removal deletes"
    );
    // A shell with no engine has observed nothing, which is not the same as "healthy".
    assert!(card.health.is_none(), "no engine, no report");
}

#[test]
fn a_loaded_plugins_card_carries_its_health() {
    // ★ The field the contract has always had and the shell never filled: every card said `None`, so a plugin the
    // quota had degraded looked exactly like one that had never run. The report comes from the same budget the gate
    // counts into, so this also proves the card and the gate are looking at one object.
    let (_root, state) = installed();
    let state = with_engine(state);
    {
        let set = state.plugin_set().expect("an engine");
        let mut set = set.lock();
        set.load(synra_core::dispatch::PluginSpec {
            id: PluginId::parse(ID).expect("an id"),
            version: "1.0.0".to_owned(),
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            grants: synra_capability::CapabilitySet::new(),
            policy: synra_plugin_host::InstancePolicy::Stateless,
            limit: 1,
            quota: synra_plugin_host::quota::Quota::default(),
        })
        .expect("loads");
    }

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    let health = card.health.as_ref().expect("a loaded plugin has a report");
    assert_eq!(health.health, "good");
    assert_eq!(health.calls_admitted, 0);
    assert!(!health.usage.is_null(), "the quota in force is carried");
}

#[test]
fn a_plugin_awaiting_consent_is_listed_and_says_so() {
    // The list is where the user finds the plugin to answer about, so the state the consent prompt is triggered by
    // has to be the state the list reports. Asserted through `plugins_list` rather than the consent command's own
    // answer, because the interface reaches this one first.
    let (_root, state) = installed();

    let listed = plugins_list(&state).expect("a list");
    let card = listed.first().expect("the installed plugin");
    assert_eq!(card.plugin_id.as_str(), ID);
    assert_eq!(card.state, "awaiting_consent");
    assert!(
        card.granted.is_empty() && card.denied.is_empty(),
        "nothing has been decided yet"
    );
}

#[test]
fn a_publisher_decision_needs_a_key_this_device_has_met() {
    // ★ "Trusted" is a decision on a row that exists because a signature was verified. A command that could create
    // the row would let the interface trust a key no package ever carried.
    let state = crate::testing::state();

    let fingerprint = "AB12 CD34 EF56 7890";
    let refused = plugins_publisher_decide(&state, "@ghost", fingerprint, "trusted")
        .expect_err("nobody has met it");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);

    state
        .db()
        .record_publisher("@acme", "AB12 CD34 EF56 7890", "a2V5", 1_000)
        .expect("the key was seen");
    let refused = plugins_publisher_decide(&state, "@acme", "1111 2222 3333 4444", "trusted")
        .expect_err("a different key must not be trusted");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
    assert_eq!(plugins_publishers(&state).expect("rows")[0].trust, "seen");
    plugins_publisher_decide(&state, "@acme", fingerprint, "trusted")
        .expect("a known key can be trusted");
    let listed = plugins_publishers(&state).expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].trust, "trusted");

    let refused = plugins_publisher_decide(&state, "@acme", fingerprint, "maybe")
        .expect_err("not a decision");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}

/// Puts a loadable plugin on disk and records the decision that lets it run.
///
/// ★ The directory comes from `Paths::plugin_dir`, the same function the installer, removal and sweep use. A fixture
/// that joined the raw id here would put the manifest somewhere the loader never looks, and the test would then be
/// asserting a layout no installation has.
fn write_loadable_files(state: &crate::state::AppState) -> String {
    let manifest = format!(
        r#"{{"id": "{ID}", "version": "1.0.0", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), &manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    manifest
}

fn loadable(state: &crate::state::AppState) {
    let manifest = write_loadable_files(state);
    state
        .db()
        .insert_plugin(
            ID,
            true,
            true,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    state
        .db()
        .record_grants(
            ID,
            &[("log:*".to_owned(), true)],
            testing::FIXED_NOW_MS,
            &synra_plugin_api::manifest_digest(&manifest),
        )
        .expect("the decisions");
}

/// A directory package for a newer version, as the registry updater unpacks one.
fn write_update_source(root: &std::path::Path, version: &str) -> std::path::PathBuf {
    let directory = root.join(format!("update-{version}"));
    std::fs::create_dir_all(&directory).expect("a package directory");
    let manifest = format!(
        r#"{{"id": "{ID}", "version": "{version}", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    directory
}

#[test]
fn the_shell_loads_an_installed_plugin_rather_than_leaving_its_set_empty() {
    // ★ The whole point of this round: before it, `with_plugins_or_refuse` built an empty `PluginSet` and never
    // loaded anything, so an installed plugin was listed, could be enabled, and was never askable. The assertion is
    // on the set itself, because that is what an intent matches against.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);

    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert_eq!(summary.len(), 1, "refused: {:?}", summary.refused);
    let id = PluginId::parse(ID).expect("an id");
    assert!(state.plugin_set().expect("an engine").lock().contains(&id));

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(card.state, "active");
    assert_eq!(card.state_reason, None, "a running plugin needs no excuse");
}

#[test]
fn safe_mode_is_visible_on_the_card_rather_than_just_an_empty_set() {
    // ★ The M4 acceptance: a plugin that crashes the process three times must not be started a fourth, and the user
    // has to be told why the plugin they can see is not running. Reaching the verdict through three reports is the
    // point — hand-building `SafeMode` would only prove that a made-up value returns false.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    let crashes = synra_observe::crash::CrashStore::at(root.path().join("crashes"));
    for offset in 0..synra_observe::crash::SAFE_MODE_THRESHOLD {
        crashes
            .write(&synra_observe::crash::CrashReport::new(
                testing::FIXED_NOW_MS + i64::try_from(offset).expect("a small offset"),
                "0.1.0",
                Some("src/plugin.rs:42".to_owned()),
                Some("the plugin panicked".to_owned()),
                None,
                None,
                None,
            ))
            .expect("the report is written");
    }
    let verdict = crashes.verdict(testing::FIXED_NOW_MS);
    assert!(
        !verdict.allows_plugins(),
        "three crashes did not reach safe mode: {verdict:?}"
    );

    let summary = load_installed_plugins(
        &state,
        &verdict,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert!(
        summary.is_empty(),
        "safe mode loaded something: {:?}",
        summary.loaded
    );
    let state = state.with_plugin_loads(summary);

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(card.state, "unavailable");
    let reason = card
        .state_reason
        .as_deref()
        .expect("safe mode must say why");
    assert!(reason.contains("crashed 3 times"), "{reason}");
}

#[test]
fn a_plugin_that_failed_to_load_says_why_on_its_card() {
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let directory = state.paths.plugin_dir(ID);
    loadable(&state);
    std::fs::remove_file(directory.join("plugin.wasm")).expect("the artifact is removed");

    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert_eq!(summary.refused.len(), 1, "{summary:?}");
    let state = state.with_plugin_loads(summary);

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    // ★ A recorded refusal is `faulted`, which the design names and the page already colours red. It used to say
    // `unavailable`, leaving the page's faulted branch unreachable and making a broken plugin look like a build
    // without an engine.
    assert_eq!(card.state, "faulted");
    let reason = card
        .state_reason
        .as_deref()
        .expect("a refusal must say why");
    assert!(reason.contains("artifact"), "{reason}");
}

#[test]
fn a_directory_copied_into_this_shells_data_dir_becomes_a_card() {
    // ★ The shell has its own `plugins/` directory, and before this it never scanned it: a directory an author
    // copied in was invisible in the window while the same directory beside the daemon appeared. Discovery records
    // and does not trust, so the card is `awaiting_consent` rather than running.
    let (_root, state) = testing::file_state();
    write_loadable_files(&state);
    assert!(
        state.db().plugins().expect("rows").is_empty(),
        "nothing is recorded yet"
    );

    let found = discover_plugins(&state, testing::FIXED_NOW_MS);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].plugin_id, ID);
    assert!(found[0].problem.is_none(), "{found:?}");
    let cards = plugins_list(&state).expect("the list");
    let card = cards.first().expect("the discovered plugin");
    // A discovered directory is written disabled and untrusted, and `disabled` is the first word: the user has not
    // switched it on, so the fact that consent has also not been answered is not the card's headline.
    assert_eq!(card.state, "disabled");
    assert!(
        !card.enabled && card.granted.is_empty(),
        "a copied directory is not an approval"
    );
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
    super::update_directory(&state, ID, &source).expect("the update is applied");

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
    let refused = super::update_directory(&state, ID, &source).expect_err("the migration refused");
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
fn retrying_a_faulted_plugin_loads_it_once_the_cause_is_gone() {
    // ★ The retry path: the row is already enabled and trusted, so the only thing that has to change is the set.
    // This is what the page's Retry button calls, and it must not need a flag to flip.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let directory = state.paths.plugin_dir(ID);
    loadable(&state);
    let artifact = directory.join("plugin.wasm");
    let bytes = std::fs::read(&artifact).expect("the artifact");
    std::fs::remove_file(&artifact).expect("the artifact is removed");
    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    let state = state.with_plugin_loads(summary);
    let cards = plugins_list(&state).expect("the list");
    assert_eq!(cards[0].state, "faulted");

    std::fs::write(&artifact, bytes).expect("the artifact is back");
    let card = super::plugins_reload(&state, ID).expect("the reload answers with the card");

    assert_eq!(
        card.state, "active",
        "the retry did not load the plugin: {card:?}"
    );
    assert!(
        state
            .plugin_set()
            .expect("an engine")
            .lock()
            .contains(&PluginId::parse(ID).expect("an id")),
        "the card says active but the set is empty"
    );
}

#[test]
fn retrying_a_plugin_that_is_not_installed_is_a_refusal() {
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let refused = super::plugins_reload(&state, ID).expect_err("no row to reload");
    assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
}

#[test]
fn a_shell_without_an_engine_says_so_on_the_card() {
    // ★ "This build cannot run plugins" and "this plugin is broken" are different answers, and only one of them
    // sends the user to the right place. A shell built without an engine is a real configuration, not a failure.
    let (_root, state) = installed();
    state
        .db()
        .set_plugin_trusted(ID, true, testing::FIXED_NOW_MS)
        .expect("trusted");

    let cards = plugins_list(&state).expect("the list");
    let card = cards.first().expect("the installed plugin");
    assert!(card.state != "active");
    assert_eq!(
        card.state_reason.as_deref(),
        Some("this build has no plugin engine")
    );
}

#[test]
fn enabling_and_disabling_move_the_plugin_in_and_out_of_the_set() {
    // ★ Enable is "activating" in the design's state machine, not a flag to be honoured at the next start. The
    // assertion is on the set because that is what decides whether a share can be answered.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    state
        .db()
        .set_plugin_enabled(ID, false, testing::FIXED_NOW_MS)
        .expect("switched off");
    let id = PluginId::parse(ID).expect("an id");

    super::plugins_enable(&state, ID).expect("it enables and loads");
    assert!(
        state.plugin_set().expect("an engine").lock().contains(&id),
        "enable did not load it"
    );

    super::plugins_disable(&state, ID).expect("it disables and unloads");
    assert!(
        !state.plugin_set().expect("an engine").lock().contains(&id),
        "disable did not unload it"
    );
}

#[test]
fn confirming_an_enabled_plugins_capabilities_loads_it_without_a_restart() {
    // ★ The consent flow can run after the switch: the user enables a plugin, is asked what it may do, and approves.
    // Before this, the set stayed empty until the next launch and the plugin looked broken for no stated reason.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    write_loadable_files(&state);
    state
        .db()
        .insert_plugin(
            ID,
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");

    let card = super::plugins_consent_respond(&state, ID, &["log:*".to_owned()])
        .expect("the decision is recorded");
    assert_eq!(
        card.state, "active",
        "the card the consent returns shows the plugin already loaded"
    );
    assert!(
        state
            .plugin_set()
            .expect("an engine")
            .lock()
            .contains(&PluginId::parse(ID).expect("an id"))
    );
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

    super::plugins_uninstall(&state, ID, false).expect("it is removed");
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
    let updates = super::plugins_updates(&restarted).expect("saved updates");

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
