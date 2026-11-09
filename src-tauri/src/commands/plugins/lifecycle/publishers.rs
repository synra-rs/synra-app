use super::*;

/// Every publisher this device has met, and what it has decided about each.
///
/// # Errors
///
/// [`IpcError::Internal`] when the table cannot be read.
pub fn plugins_publishers(state: &AppState) -> Result<Vec<synra_ipc::dto::PublisherDto>, IpcError> {
    let rows = {
        let db = state.db();
        db.publishers().map_err(internal)?
    };
    Ok(rows
        .into_iter()
        .map(|row| synra_ipc::dto::PublisherDto {
            publisher_id: row.publisher_id,
            fingerprint: row.fingerprint,
            trust: row.trust,
            first_seen_ms: row.first_seen_ms,
            decided_ms: row.decided_ms,
        })
        .collect())
}

/// Decides what this device thinks of a publisher's key.
///
/// # The three answers
///
/// `trusted` is the user saying "I will take packages signed by this key"; `revoked` is the opposite; `seen` returns a
/// decision to "met but not judged", which is what a user who changes their mind gets. Nothing here reads a signature:
/// the row exists because an install verified one, and this is the judgement on top of that fact.
///
/// # Errors
///
/// [`IpcError::InvalidArgument`] for an unknown trust value, publisher, or a stale fingerprint.
pub fn plugins_publisher_decide(
    state: &AppState,
    publisher_id: &str,
    fingerprint: &str,
    trust: &str,
) -> Result<(), IpcError> {
    let row = synra_core::publishers::decide(
        &state.db(),
        publisher_id,
        fingerprint,
        trust,
        synra_store::sqlite::now_ms(),
    )
    .map_err(|error| {
        IpcError::unknown(error.code()).with_params(serde_json::json!({"reason":error.to_string()}))
    })?;
    if row.trust == synra_store::TRUST_REVOKED {
        // ★ A revocation is about the code that is already here, not only about the next install. The version row carries
        // the signing key (migration 16), so this is an index lookup rather than a scan of every installed manifest — and
        // it is the key that was revoked, not every plugin by the same publisher.
        stop_signer_plugins(state, &row.fingerprint);
    }
    Ok(())
}

/// Disables and unloads every active plugin the revoked key signed.
///
/// Failures are logged rather than returned: the decision **is** recorded, and answering an error would tell the user the
/// revocation did not happen. A plugin that could not be stopped keeps its row `enabled = 0`, so the next start does not
/// bring it back.
fn stop_signer_plugins(state: &AppState, fingerprint: &str) {
    let Ok(plugins) = state.db().active_plugins_by_fingerprint(fingerprint) else {
        return;
    };
    for plugin_id in plugins {
        let _ = state
            .db()
            .set_plugin_trusted(&plugin_id, false, synra_store::sqlite::now_ms());
        if let Err(error) = set_enabled(state, &plugin_id, false) {
            eprintln!("synra: the revoked plugin {plugin_id} could not be stopped: {error:?}");
        }
    }
}
