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
    let wasm_bytes = synra_testkit::component::inert_component().expect("a valid component");
    let wasm = wasm_bytes.as_slice();
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
    let fingerprint = state.db().publishers().expect("publishers")[0]
        .fingerprint
        .clone();
    super::super::plugins::plugins_publisher_decide(&state, "@acme", &fingerprint, "trusted")
        .expect("trust publisher");
    super::super::plugins::plugins_consent_respond(&state, "@acme/hello", &["log:*".to_owned()])
        .expect("consent");
    super::super::plugins::plugins_enable(&state, "@acme/hello").expect("enabled");
    (root, state)
}

mod authorization;
mod automatic;
mod retries;
