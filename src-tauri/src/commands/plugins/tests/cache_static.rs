use super::*;

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
