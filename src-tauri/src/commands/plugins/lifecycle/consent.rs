use super::*;

/// Records the user's decision about what a plugin may do, and returns its card as it now stands.
///
/// # ★ Why this command is the one that was missing
///
/// `plugin_grants` — the table this writes — had a schema, an index, a row type, a reader, and **no writer**. The
/// settings screen showed two empty lists, and the daemon's loader took the manifest's own declaration as the grant,
/// because that was the only thing left to take. A plugin received exactly what it asked for, and the per-capability
/// consent the design rests on did not exist at either end.
///
/// # The rule for `trusted`
///
/// **Anything allowed is approval.** A decision that grants one capability and refuses two means the plugin runs with
/// that one and is refused at the gate for the other two — which is what a capability gate is for, and what the design's
/// `Consent::is_partial()` describes. A decision that refuses everything leaves the plugin unable to run.
///
/// ★ **This used to be the opposite here** — trusted only when *every* declared capability was granted — while the
/// daemon used "any". The loader then took the granted rows as the authority and loaded either way, so one user
/// decision produced a plugin that could not run in this host and one that ran with a subset in the other. The rule now
/// lives in `synra_core::consent`, and both hosts ask it.
///
/// # Why the answer is the card rather than a bare success
///
/// Because the caller's next act depends on it: a plugin that became trusted is one the interface shows as ready, and
/// one that did not needs the reason. Re-reading the list would answer the same question a moment later and about every
/// plugin, which is a worse trade than returning the one that changed.
///
/// # Errors
///
/// * [`IpcError::NotFound`] when no plugin of that identifier is installed.
/// * [`IpcError::Internal`] when the manifest cannot be read or a decision cannot be written.
///
/// A manifest that is missing or unreadable is refused rather than treated as declaring nothing: a plugin whose
/// capabilities cannot be read is one whose consent cannot be obtained, and recording an empty decision set would look
/// exactly like a user who declined everything.
pub fn plugins_consent_respond(
    state: &AppState,
    plugin_id: &str,
    granted: &[String],
) -> Result<PluginSnapshot, IpcError> {
    let id = parse(plugin_id)?;
    let text = read_manifest_text(state, &id.to_string()).ok_or_else(|| {
        IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id}))
    })?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(internal)?;

    // ★ What the decision means is the kernel's rule rather than this command's — see the note above about the two
    // hosts having answered it differently.
    let decision = synra_core::consent::decide(&manifest, &text, granted).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })?;

    state
        .db()
        .record_grants(
            &id.to_string(),
            &decision.pairs(),
            state.started_at_ms,
            decision.digest(),
        )
        .map_err(internal)?;

    if !state
        .db()
        .set_plugin_trusted(&id.to_string(), decision.trusted(), state.started_at_ms)
        .map_err(internal)?
    {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id})),
        );
    }

    // ★ A plugin can be enabled *before* it is trusted — the switch is a preference and consent is the gate — so
    // recording the decision may be the moment it becomes runnable. Loading here is what makes that true immediately
    // rather than at the next start. A refusal is recorded on the load report by `apply_lifecycle`, and the card below
    // carries it; the consent itself succeeded, so this command still answers with the card.
    let _ = apply_lifecycle(state, &id);

    let row = state
        .db()
        .plugins()
        .map_err(internal)?
        .into_iter()
        .find(|row| row.plugin_id == id.to_string())
        .ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id}))
        })?;
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    let card = snapshot(state, &row, &id, loaded.as_deref())?;
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(
        card.clone(),
    )));
    Ok(card)
}
