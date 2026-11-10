use super::*;

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
