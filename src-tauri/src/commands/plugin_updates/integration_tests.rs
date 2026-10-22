//! The real registry request, saved schedule, restart and offline retry together.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Registry {
    base: String,
    body: Arc<parking_lot::Mutex<String>>,
    tarball: Arc<parking_lot::Mutex<Vec<u8>>>,
    requests: Arc<AtomicUsize>,
    cancel: Cancellation,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Registry {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("port");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let body = Arc::new(parking_lot::Mutex::new(document("2.0.0")));
        let tarball = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let requests = Arc::new(AtomicUsize::new(0));
        let cancel = Cancellation::new();
        let reply = Arc::clone(&body);
        let archive = Arc::clone(&tarball);
        let counted = Arc::clone(&requests);
        let stop = cancel.clone();
        let worker = std::thread::spawn(move || {
            while !stop.is_cancelled() {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).expect("blocking connection");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .expect("read deadline");
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .expect("write deadline");
                        let mut reader = BufReader::new(&mut stream);
                        let mut request = String::new();
                        loop {
                            let mut line = String::new();
                            let read = reader.read_line(&mut line).expect("request header");
                            request.push_str(&line);
                            assert!(request.len() < 8192, "bounded request headers");
                            if read == 0 || line == "\r\n" {
                                break;
                            }
                        }
                        let body = if request.starts_with("GET /hello.tgz ") {
                            archive.lock().clone()
                        } else {
                            reply.lock().as_bytes().to_vec()
                        };
                        counted.fetch_add(1, Ordering::SeqCst);
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            body.len()
                        )
                        .expect("response");
                        stream.write_all(&body).expect("body");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop.wait(Duration::from_millis(2)) {
                            break;
                        }
                    }
                    Err(error) => panic!("registry accept: {error}"),
                }
            }
        });
        Self {
            base,
            body,
            tarball,
            requests,
            cancel,
            worker: Some(worker),
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn document(version: &str) -> String {
    serde_json::json!({
        "name": "@acme/hello",
        "dist-tags": { "latest": version },
        "versions": {
            version: { "name": "@acme/hello", "version": version,
                "dist": { "tarball": "https://registry.example/hello.tgz" } }
        }
    })
    .to_string()
}

fn signed_source(directory: &std::path::Path, version: &str, seed: u8, permissions: u8) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use synra_plugin_api::publisher::{Digests, SigningSubject};

    std::fs::create_dir_all(directory).expect("source");
    let capabilities = match permissions {
        0 => serde_json::json!([{"cap":"log:*"}]),
        1 => serde_json::json!([{"cap":"log:*"}, {"cap":"device:send:*"}]),
        _ => serde_json::json!([]),
    };
    let manifest = serde_json::json!({
        "id":"@acme/hello", "version":version, "title":"Hello", "publisher":{"id":"@acme"},
        "sdk":{"min":"0.1.0"}, "wit":synra_plugin_host::host_world(), "capabilities":capabilities,
        "runtime":{"headless":{"artifact":"plugin.wasm", "stateful":false}}
    })
    .to_string();
    let wasm = b"\0asm\x01\0\0\0";
    let mut artifacts = Digests::new();
    artifacts.insert(
        "plugin.wasm",
        synra_core::transfer::ObjectDigest::of(wasm)
            .as_sri()
            .to_string(),
    );
    let subject = SigningSubject {
        manifest: synra_plugin_api::manifest_digest(&manifest),
        artifacts,
    };
    let key = SigningKey::from_bytes(&[seed; 32]);
    let signature = serde_json::json!({
        "version":1, "publisher":{"id":"@acme"}, "algorithm":"ed25519", "signedAt":"2026-10-22T00:00:00Z",
        "pubkey":STANDARD.encode(key.verifying_key().as_bytes()),
        "subject":{"manifest":subject.manifest, "artifacts":subject.artifacts.iter().collect::<std::collections::BTreeMap<_,_>>()},
        "signature":STANDARD.encode(key.sign(&subject.canonical_bytes()).to_bytes())
    });
    std::fs::write(directory.join("manifest.json"), manifest).expect("manifest");
    std::fs::write(directory.join("plugin.wasm"), wasm).expect("wasm");
    std::fs::write(directory.join("signature.json"), signature.to_string()).expect("signature");
}

fn publish(registry: &Registry, source: &std::path::Path, version: &str) {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for file in ["manifest.json", "plugin.wasm", "signature.json"] {
        tar.append_path_with_name(source.join(file), format!("package/{file}"))
            .expect("entry");
    }
    let bytes = tar.into_inner().expect("tarball").finish().expect("gzip");
    let integrity = synra_core::transfer::ObjectDigest::of(&bytes)
        .as_sri()
        .to_string();
    *registry.tarball.lock() = bytes;
    // Deliberately omit the inline manifest. Admission must use the downloaded manifest, never an advertised permission
    // list: even a registry that hides an added permission cannot cause it to be installed automatically.
    *registry.body.lock() = serde_json::json!({
        "name":"@acme/hello", "dist-tags":{"latest":version},
        "versions": {version:{"name":"@acme/hello", "version":version,
            "dist":{"tarball":format!("{}/hello.tgz", registry.base), "integrity":integrity}}}
    })
    .to_string();
}

fn installed_state() -> (crate::testing::Root, AppState) {
    let (root, state) = crate::testing::file_state();
    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let backend =
        synra_plugin_host::Backend::new(Arc::new(synra_plugin_host::ScriptedBackend::new()));
    let state = state.with_plugins(Arc::new(parking_lot::Mutex::new(
        synra_core::dispatch::PluginSet::new(backend, host),
    )));
    let first = root.path().join("first");
    signed_source(&first, "1.0.0", 7, 0);
    super::super::plugins::install_directory(&state, &first).expect("first install");
    let fingerprint = state.db().publishers().expect("publishers")[0].fingerprint.clone();
    super::super::plugins::plugins_publisher_decide(&state, "@acme", &fingerprint, "trusted")
        .expect("trust publisher");
    super::super::plugins::plugins_consent_respond(&state, "@acme/hello", &["log:*".to_owned()])
        .expect("consent");
    super::super::plugins::plugins_enable(&state, "@acme/hello").expect("enabled");
    (root, state)
}

#[test]
fn cancellation_policy_changes_and_mismatching_targets_leave_the_running_version_alone() {
    let (root, state) = installed_state();
    let next = root.path().join("next");
    signed_source(&next, "1.1.0", 7, 0);
    for (selected, cancelled, id, version) in [
        ("auto-all", true, "@acme/hello", "1.1.0"),
        ("never", false, "@acme/hello", "1.1.0"),
        ("auto-all", false, "@acme/other", "1.1.0"),
        ("auto-all", false, "@acme/hello", "2.0.0"),
    ] {
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!(selected), 1)
            .expect("policy");
        let cancel = Cancellation::new();
        if cancelled {
            cancel.cancel();
        }
        assert!(
            super::apply_automatic_update(
                &state,
                id,
                version,
                &next,
                None,
                UpdatePolicy::AutoAll,
                &cancel
            )
            .is_err()
        );
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("version")
                .expect("installed")
                .version,
            "1.0.0"
        );
        assert_eq!(
            state
                .plugin_set()
                .expect("engine")
                .lock()
                .version(&PluginId::parse("@acme/hello").expect("id")),
            Some("1.0.0")
        );
    }
}

#[test]
fn a_changed_registry_or_revoked_candidate_leaves_the_running_version_untouched() {
    for revoked in [false, true] {
        let (root, state) = installed_state();
        let next = root.path().join("next");
        signed_source(&next, "1.1.0", 7, 0);
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!("auto-all"), 1)
            .expect("policy");
        state
            .db()
            .set_config(
                super::super::registry::REGISTRY_KEY,
                &serde_json::json!("https://original.example"),
                1,
            )
            .expect("source");
        let expected =
            synra_core::plugin_updates::registry_setting(&state.db()).expect("source snapshot");
        if revoked {
            std::fs::write(state.paths.data_dir().join("revocations.json"), r#"{"revoked":[{"plugin":"@acme/hello","versions":["1.1.0"],"reason":"withdrawn"}]}"#).expect("revocations");
        } else {
            state
                .db()
                .set_config(
                    super::super::registry::REGISTRY_KEY,
                    &serde_json::json!("https://changed.example"),
                    1,
                )
                .expect("changed source");
        }
        assert!(
            super::apply_automatic_update(
                &state,
                "@acme/hello",
                "1.1.0",
                &next,
                expected.as_deref(),
                UpdatePolicy::AutoAll,
                &Cancellation::new()
            )
            .is_err()
        );
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("row")
                .expect("installed")
                .version,
            "1.0.0"
        );
        assert_eq!(
            state
                .plugin_set()
                .expect("engine")
                .lock()
                .version(&PluginId::parse("@acme/hello").expect("id")),
            Some("1.0.0")
        );
    }
}

#[test]
fn the_daily_pass_applies_only_the_updates_the_user_and_verified_package_allow() {
    for (version, selected, seed, added, applies) in [
        ("1.0.1", "auto-minor", 7, 0, true),
        ("2.0.0", "auto-all", 7, 0, true),
        ("1.1.0", "auto-minor", 7, 2, true),
        ("2.0.0", "auto-minor", 7, 0, false),
        ("1.1.0", "auto-all", 9, 0, false),
        ("1.1.0", "auto-all", 7, 1, false),
        ("1.1.0", "notify-only", 7, 0, false),
        ("1.1.0", "never", 7, 0, false),
    ] {
        let registry = Registry::start();
        let (root, state) = installed_state();
        let next = root.path().join("next");
        signed_source(&next, version, seed, added);
        publish(&registry, &next, version);
        state
            .db()
            .set_config(
                super::super::registry::REGISTRY_KEY,
                &serde_json::json!(registry.base),
                1,
            )
            .expect("registry");
        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::json!(selected), 1)
            .expect("policy");
        let subscriber = state.events.subscribe();
        let updates = check_all(&state, 1, &Cancellation::new());
        let expected = if applies { version } else { "1.0.0" };
        assert_eq!(
            state
                .db()
                .active_plugin("@acme/hello")
                .expect("row")
                .expect("installed")
                .version,
            expected,
            "{selected}: {version}, seed {seed}, added {added}"
        );
        let id = PluginId::parse("@acme/hello").expect("id");
        assert_eq!(
            state.plugin_set().expect("engine").lock().version(&id),
            Some(expected),
            "running version agrees with disk"
        );
        assert_eq!(updates.is_empty(), applies || selected == "never");
        let events = state.events.take(subscriber);
        assert_eq!(events.as_ref().is_some_and(|events| events.events().iter().any(|event| matches!(event, KernelEvent::Notice { message, .. } if message == "notice.updated"))), applies);
        if selected == "notify-only" {
            assert_eq!(registry.requests.load(Ordering::SeqCst), 1);
        }
        if selected == "never" {
            assert_eq!(registry.requests.load(Ordering::SeqCst), 0);
        }
        if applies && added == 2 {
            signed_source(&next, "1.2.0", seed, 2);
            publish(&registry, &next, "1.2.0");
            assert!(
                check_all(
                    &state,
                    synra_store::sqlite::now_ms() + CHECK_EVERY_MS,
                    &Cancellation::new()
                )
                .is_empty()
            );
            assert_eq!(
                state.plugin_set().expect("engine").lock().version(&id),
                Some("1.2.0"),
                "consecutive permissionless updates keep running"
            );
        }
        let cache = state.paths.cache_dir().join("registry");
        assert!(
            !cache.exists() || cache.read_dir().expect("cache").next().is_none(),
            "download cleaned up"
        );
    }
}

#[test]
fn a_restart_keeps_daily_checks_and_an_offline_attempt_keeps_the_last_answer() {
    let registry = Registry::start();
    let (_root, state) = crate::testing::file_state();
    let plugin = "@acme/hello";
    state
        .db()
        .insert_plugin(plugin, false, false, "good", 1, 1)
        .expect("plugin");
    state
        .db()
        .record_plugin_version(plugin, "1.0.0", "wasm", "digest", "{}", 1)
        .expect("version");
    state
        .db()
        .set_config(
            super::super::registry::REGISTRY_KEY,
            &serde_json::json!(registry.base),
            1,
        )
        .expect("registry");
    let cancel = Cancellation::new();
    let updates = check_all(&state, 1, &cancel);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].version, "2.0.0");
    let saved = state
        .db()
        .plugin_update_check(plugin)
        .expect("check")
        .expect("saved");
    let restarted = crate::testing::reopen(&state);
    assert!(
        check_all(
            &restarted,
            saved.checked_at_ms + CHECK_EVERY_MS - 1,
            &cancel
        )
        .is_empty()
    );
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        1,
        "restart does not recheck early"
    );
    assert_eq!(
        super::super::plugins::plugins_updates(&restarted).expect("updates"),
        updates
    );

    *registry.body.lock() = "unreadable registry response".to_owned();
    assert!(check_all(&restarted, saved.checked_at_ms + CHECK_EVERY_MS, &cancel).is_empty());
    assert_eq!(registry.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        restarted.db().plugin_update_check(plugin).expect("check"),
        Some(saved.clone())
    );

    *registry.body.lock() = document("1.0.0");
    assert!(check_all(&restarted, saved.checked_at_ms + CHECK_EVERY_MS, &cancel).is_empty());
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        3,
        "failure is retried at the next pass"
    );
    assert!(
        super::super::plugins::plugins_updates(&restarted)
            .expect("updates")
            .is_empty()
    );
    let negative = restarted
        .db()
        .plugin_update_check(plugin)
        .expect("check")
        .expect("negative check");
    assert!(negative.available_version.is_none());
    let restarted_again = crate::testing::reopen(&restarted);
    assert!(check_all(&restarted_again, negative.checked_at_ms + 1, &cancel).is_empty());
    assert_eq!(
        registry.requests.load(Ordering::SeqCst),
        3,
        "negative checks also survive a restart"
    );
}
