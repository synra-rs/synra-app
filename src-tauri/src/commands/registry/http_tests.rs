use super::*;
use crate::testing::file_state;
use serde_json::json;

fn query() -> CatalogQuery {
    CatalogQuery {
        query: "reader".to_owned(),
        ..CatalogQuery::default()
    }
}

#[test]
fn real_http_failures_distinguish_an_absent_search_api_from_transient_network_failures() {
    for (status, code) in [
        (404, ErrorCode::Unsupported),
        (408, ErrorCode::Timeout),
        (429, ErrorCode::RateLimited),
        (503, ErrorCode::Internal),
        (403, ErrorCode::InvalidArgument),
    ] {
        let (address, server) = super::test_server::respond(status, "refused");
        let (_root, state) = file_state();
        state
            .db()
            .set_config(REGISTRY_KEY, &json!(address), 1)
            .expect("configured endpoint");
        let error = search(&state, &query()).expect_err("server refusal");
        assert_eq!(error.code, code);
        assert_eq!(error.params["status"], status);
        assert_eq!(error.retryable, code.retryable());
        if status == 404 {
            assert_eq!(
                error.details,
                Some(json!({"registry": {"kind": "searchUnsupported"}}))
            );
        } else {
            assert!(error.details.is_none());
        }
        if error.retryable {
            assert!(
                error
                    .actions
                    .contains(&synra_protocol::error::SuggestedAction::Retry)
            );
        }
        assert!(
            server
                .join()
                .expect("request completed")
                .starts_with("GET /-/v1/search?")
        );
    }
}

#[test]
fn a_temporary_mirror_failure_can_reach_a_second_source_with_an_honest_empty_result() {
    let (first, first_server) = super::test_server::respond(503, "refused");
    let (second, second_server) = super::test_server::respond(200, "{\"objects\":[]}");
    let (_root, state) = file_state();
    state
        .db()
        .set_config(REGISTRY_KEY, &json!([first, second]), 1)
        .expect("ordered mirrors");
    assert!(search(&state, &query()).expect("second mirror").is_empty());
    assert!(
        first_server
            .join()
            .expect("first request")
            .contains("/-/v1/search?")
    );
    assert!(
        second_server
            .join()
            .expect("second request")
            .contains("/-/v1/search?")
    );
}

#[test]
fn an_integrity_failure_never_downloads_from_the_next_mirror() {
    use base64::Engine as _;
    let (first, server) = super::test_server::respond(200, "bytes with the wrong digest");
    let untouched =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("second mirror");
    untouched
        .set_nonblocking(true)
        .expect("inspect without blocking");
    let second = format!("http://{}", untouched.local_addr().expect("mirror address"));
    let (_root, state) = file_state();
    state
        .db()
        .set_config(REGISTRY_KEY, &json!([first, second]), 1)
        .expect("ordered mirrors");
    let bases = bases(&state).expect("sources");
    let chosen: Version = serde_json::from_value(json!({
        "name": "@acme/reader", "version": "1.0.0", "dist": {
            "tarball": format!("{first}/reader.tgz"),
            "integrity": format!("sha256-{}", base64::engine::general_purpose::STANDARD.encode([0; 32])),
        }
    })).expect("valid package metadata and SRI encoding");
    let error = download(&state, &bases, &bases[0], &chosen, None)
        .err()
        .expect("integrity refusal");
    assert_eq!(error.code, ErrorCode::IntegrityMismatch);
    assert!(
        server
            .join()
            .expect("first download")
            .starts_with("GET /reader.tgz ")
    );
    assert_eq!(
        untouched
            .accept()
            .err()
            .expect("no fallback connection")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}
