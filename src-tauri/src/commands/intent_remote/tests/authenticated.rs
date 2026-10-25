use super::*;
use synra_core::peer::authenticated::AuthenticatedPeer;
use synra_testkit::authentication::NetworkFixture;

fn state(fixture: &NetworkFixture, address: std::net::SocketAddr) -> AppState {
    let state = AppState::new(
        fixture.open_connection().unwrap(),
        synra_store::Paths::under(fixture.root()),
        "test",
        fixture.now_ms as i64,
    )
    .unwrap();
    let identity = state.identity.lock().clone();
    let network = AuthenticatedPeer::new(
        Arc::clone(&state.db),
        Arc::clone(&fixture.credentials),
        crate::commands::transfers::local_peer(&identity),
    )
    .unwrap();
    state.host.lock().configure_network(Ok(network));
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: PEER.to_owned(),
            display_name: "Peer".to_owned(),
            platform: "desktop".to_owned(),
            features: "[]".to_owned(),
            link_token: "metadata-only".to_owned(),
            address: Some(address.to_string()),
            paired_at_ms: fixture.now_ms as i64,
        })
        .unwrap();
    state
}

#[test]
fn the_production_app_delivery_entry_uses_authenticated_hello_and_business_frames() {
    let local = NetworkFixture::new(device(LOCAL), device(PEER), "Local").unwrap();
    let peer = NetworkFixture::new(device(PEER), device(LOCAL), "Peer").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let state = state(&local, listener.local_addr().unwrap());
    let remote = AuthenticatedPeer::new(
        Arc::clone(&peer.shared_db),
        Arc::clone(&peer.credentials),
        synra_core::LocalPeer::new(
            device(PEER),
            "Peer",
            synra_protocol::session::PlatformKind::Desktop,
            "0.1.0",
        ),
    )
    .unwrap();
    let outbound = invitation(RequestId::new());
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let (socket, _) = listener.accept().unwrap();
            let (mut link, _) = remote
                .accept(
                    socket,
                    peer.now_ms,
                    peer.now_ms / 1000,
                    Duration::from_secs(3),
                )
                .unwrap();
            let request = link.receive().unwrap();
            assert_eq!(request.from, device(LOCAL));
            let mut reply = remote::result(
                device(PEER),
                device(LOCAL),
                &InvokeResult {
                    ok: true,
                    result: "authenticated".to_owned(),
                    error: None,
                    error_code: None,
                    elapsed_ms: 1,
                },
                request.trace_id,
                peer.now_ms as i64,
            )
            .unwrap();
            reply.reply_to = Some(request.request_id);
            link.send(&reply).unwrap();
        });
        let answer = deliver(&state, &outbound, Some(Duration::from_secs(3)))
            .unwrap()
            .unwrap();
        assert_eq!(answer.reply_to, Some(outbound.request_id));
        worker.join().unwrap();
    });
    assert!(
        state
            .metrics
            .snapshot()
            .get("frames_sent_total{event=_synra.intent.invoke}")
            .is_some()
    );
}

#[test]
fn old_pairing_metadata_cannot_authorize_an_app_dial_after_credential_revocation() {
    let local = NetworkFixture::new(device(LOCAL), device(PEER), "Local").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let state = state(&local, listener.local_addr().unwrap());
    state.db().unpair_device(PEER).unwrap();
    // Restoring only list metadata and a bearer token cannot restore the credential grant.
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: PEER.to_owned(),
            display_name: "Peer".to_owned(),
            platform: "desktop".to_owned(),
            features: "[]".to_owned(),
            link_token: "old-token".to_owned(),
            address: Some(listener.local_addr().unwrap().to_string()),
            paired_at_ms: local.now_ms as i64,
        })
        .unwrap();
    assert!(deliver(&state, &invitation(RequestId::new()), None).is_err());
    assert!(
        listener
            .accept()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
    );
}
