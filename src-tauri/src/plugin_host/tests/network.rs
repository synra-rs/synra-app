use std::sync::Arc;

use synra_plugin_host::host_calls::{CallError, Host as _};
use synra_protocol::ErrorCode;

use super::{host_with, id};

#[test]
fn a_cached_client_reads_current_host_and_method_grants() {
    let host = host_with(&[("network:fetch:api.example.test:GET", true)]);
    let plugin = id("@acme/notes");
    let before = host.fetch_clients.get(&plugin);
    let read = r#"{"url":"http://api.example.test/","method":"GET"}"#;
    let write = r#"{"url":"http://api.example.test/","method":"POST"}"#;
    assert_eq!(
        host.fetch(&plugin, read).unwrap_err().code,
        ErrorCode::SchemeNotAllowed.as_str()
    );
    assert_eq!(
        host.fetch(&plugin, write).unwrap_err().code,
        CallError::CAPABILITY_DENIED
    );

    host.db
        .lock()
        .record_grants(
            plugin.as_str(),
            &[
                ("network:fetch:api.example.test:GET".to_owned(), false),
                ("network:fetch:another.example.test:POST".to_owned(), true),
            ],
            2,
            "scope change",
        )
        .unwrap();

    assert!(Arc::ptr_eq(&before, &host.fetch_clients.get(&plugin)));
    assert_eq!(
        host.fetch(&plugin, read).unwrap_err().code,
        CallError::CAPABILITY_DENIED
    );
    let new_scope = r#"{"url":"http://another.example.test/","method":"POST"}"#;
    assert_eq!(
        host.fetch(&plugin, new_scope).unwrap_err().code,
        ErrorCode::SchemeNotAllowed.as_str()
    );
}
