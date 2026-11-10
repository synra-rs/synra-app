use super::*;

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
