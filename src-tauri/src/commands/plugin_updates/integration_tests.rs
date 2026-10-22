//! The real registry request, saved schedule, restart and offline retry together.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Registry {
    base: String,
    body: Arc<parking_lot::Mutex<String>>,
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
        let requests = Arc::new(AtomicUsize::new(0));
        let cancel = Cancellation::new();
        let reply = Arc::clone(&body);
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
                        let body = reply.lock().clone();
                        counted.fetch_add(1, Ordering::SeqCst);
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .expect("response");
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
