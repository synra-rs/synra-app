use super::*;
use synra_plugin_host::host_calls::Host;

#[test]
fn a_cached_client_rechecks_stored_method_grants_for_each_request() {
    let host = tests::host_with(&[("network:fetch:127.0.0.1:GET", true)]);
    let plugin = PluginId::parse("@acme/notes").unwrap();
    let before = fetcher_for(&host, &plugin).unwrap();
    let error = host
        .fetch(&plugin, r#"{"url":"https://127.0.0.1:1/","method":"POST"}"#)
        .unwrap_err();
    assert_eq!(error.code, CallError::CAPABILITY_DENIED);
    host.db
        .lock()
        .record_grants(
            plugin.as_str(),
            &[
                ("network:fetch:127.0.0.1:GET".to_owned(), false),
                ("network:fetch:127.0.0.1:POST".to_owned(), true),
            ],
            2,
            "method change",
        )
        .unwrap();
    assert!(Arc::ptr_eq(&before, &fetcher_for(&host, &plugin).unwrap()));
    let error = host
        .fetch(&plugin, r#"{"url":"https://127.0.0.1:1/","method":"GET"}"#)
        .unwrap_err();
    assert_eq!(error.code, CallError::CAPABILITY_DENIED);
}
