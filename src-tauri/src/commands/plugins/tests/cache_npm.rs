use super::*;

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
