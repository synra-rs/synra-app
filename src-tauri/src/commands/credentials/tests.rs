use super::*;

#[test]
fn missing_enrollment_is_explicit_and_does_not_generate_a_key() {
    let state = crate::testing::state();
    let status = identity_credentials(&state).unwrap();
    assert!(status.enrollment.is_none());
    assert!(!status.runtime_ready);
    assert!(!status.restart_required);
}

#[cfg(windows)]
#[test]
fn native_app_recovery_revokes_metadata_and_reports_restart_separately_from_enrollment() {
    use std::sync::Arc;
    use synra_core::peer::authenticated::AuthenticatedPeer;
    use synra_store::secrets::{SecretBackend, SecretKey, WindowsCredentialBackend};
    use synra_testkit::authentication::NetworkFixture;

    struct Cleanup(
        Arc<parking_lot::Mutex<synra_store::SynraDb>>,
        synra_protocol::DeviceId,
    );
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let backend = WindowsCredentialBackend::new(
                synra_core::device_credentials::DEVICE_IDENTITY_SERVICE,
            )
            .unwrap();
            if let Ok(credentials) = synra_core::device_credentials::load(
                &self.0,
                &backend,
                self.1,
                synra_store::sqlite::now_ms() as u64 / 1000,
            ) {
                backend
                    .delete(&SecretKey::device_identity(
                        *credentials.public_identity().fingerprint(),
                    ))
                    .unwrap();
            }
        }
    }

    let id = synra_protocol::DeviceId::new();
    let fixture = NetworkFixture::new(id, synra_protocol::DeviceId::new(), "Repair").unwrap();
    let _cleanup = Cleanup(Arc::clone(&fixture.shared_db), id);
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
    assert_eq!(state.db().paired_devices().unwrap().len(), 1);
    let before = identity_credentials(&state).unwrap();
    assert!(before.runtime_ready);
    let old = before.enrollment.unwrap().fingerprint;
    let connection =
        rusqlite::Connection::open(fixture.root().join(synra_store::DB_FILE_NAME)).unwrap();
    connection
        .execute("DELETE FROM device_credentials", [])
        .unwrap();
    let lost = identity_credentials(&state).unwrap();
    assert!(lost.enrollment.is_none());
    assert_eq!(lost.recovery.unwrap().fingerprint, old);
    assert!(!lost.runtime_ready);
    assert_eq!(
        identity_repair(&state, "stale").unwrap_err().code,
        synra_protocol::ErrorCode::InvalidArgument
    );
    let report = identity_repair(&state, &old).unwrap();
    assert!(report.restart_required);
    assert_ne!(report.fingerprint, old);
    assert!(state.db().paired_devices().unwrap().is_empty());
    let after = identity_credentials(&state).unwrap();
    assert!(!after.runtime_ready);
    assert!(after.restart_required);
    assert!(after.enrollment.as_ref().unwrap().ready);
    assert_eq!(after.enrollment.unwrap().fingerprint, report.fingerprint);
    assert_eq!(identity_cleanup(&state).unwrap().remaining, Some(1));
    assert!(identity_repair(&state, &old).is_err());
}

#[test]
fn lost_registration_and_history_report_that_a_trusted_backup_is_required() {
    let fixture = synra_testkit::authentication::NetworkFixture::unpaired(
        synra_protocol::DeviceId::new(),
        "Lost history",
    )
    .unwrap();
    let state = AppState::new(
        fixture.open_connection().unwrap(),
        synra_store::Paths::under(fixture.root()),
        "test",
        fixture.now_ms as i64,
    )
    .unwrap();
    let connection =
        rusqlite::Connection::open(fixture.root().join(synra_store::DB_FILE_NAME)).unwrap();
    connection
        .execute_batch("DELETE FROM device_credentials; DELETE FROM device_identity_history;")
        .unwrap();
    let error = identity_credentials(&state).unwrap_err();
    assert_eq!(error.code, synra_protocol::ErrorCode::NotPaired);
    assert_eq!(error.message.as_str(), "error.identityHistoryMissing");
}
