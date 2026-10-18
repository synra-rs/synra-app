//! The plugins this device has.
//!
//! # Installed, enabled, trusted and loaded are four different questions
//!
//! The list the interface shows is of *installed* plugins — what is on disk. Whether it is **enabled** is a decision the
//! user made. Whether it is **trusted** is a decision the user made about what it may do, and it is stored separately on
//! purpose: re-consenting after an upgrade is not the same act as switching a plugin on, and a plugin that silently
//! starts running is exactly what consent exists to prevent. Whether it is **loaded** is what this process managed to
//! do with its engine.
//!
//! `PluginSnapshot::state` carries the kernel's own answer to "where is it in its life", and the shell does not derive a
//! second state from the other three flags — the design document (`08` §2.2) already exists so that this rule has one
//! implementation, and a second one in the shell is where the two start disagreeing.
//!
//! # What `plugins_list` never does
//!
//! It never filters out a plugin because it failed to load. A plugin that is installed and broken is the one a user most
//! needs to see: they will go looking for the setting to remove it, and if it is not on the screen they will file a bug
//! about it having vanished.

use synra_ipc::{
    AuditEntryDto, CatalogItem, CatalogQuery, InstallPreviewDto, InstallReport, InstallSourceDto, IpcError,
    PluginSnapshot, UpdateInfo,
};
use synra_plugin_api::Manifest;
use synra_protocol::PluginId;

use crate::state::{AppState, internal};

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
        .record_grants(&id.to_string(), &decision.pairs(), state.started_at_ms, decision.digest())
        .map_err(internal)?;

    if !state.db().set_plugin_trusted(&id.to_string(), decision.trusted(), state.started_at_ms).map_err(internal)? {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id})));
    }

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
    let card = snapshot(state, &row, &id, loaded.as_deref());
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(card.clone())));
    Ok(card)
}

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
/// [`IpcError::InvalidArgument`] for an unknown trust value or a publisher this device has not met.
pub fn plugins_publisher_decide(
    state: &AppState,
    publisher_id: &str,
    trust: &str,
) -> Result<(), IpcError> {
    use synra_protocol::ErrorCode;

    let trust = match trust {
        "trusted" => synra_store::TRUST_TRUSTED,
        "seen" => synra_store::TRUST_SEEN,
        "revoked" => synra_store::TRUST_REVOKED,
        other => {
            return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "field": "trust",
                "reason": format!("`{other}` is not one of seen, trusted, revoked"),
            })))
        }
    };
    if publisher_id.is_empty() || publisher_id.len() > 256 {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": "publisherId", "reason": "it must be 1–256 bytes"})));
    }
    let changed = {
        let db = state.db();
        db.set_publisher_trust(publisher_id, trust, synra_store::sqlite::now_ms())
            .map_err(internal)?
    };
    if changed {
        Ok(())
    }
    else {
        Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"publisherId": publisher_id, "reason": "no signature from it was seen"})))
    }
}

/// Installs a `.synra` package a peer sent, with this shell's engine.
///
/// # Why the backend is cloned before the install
///
/// Because the plugin set's lock is shared with every command that asks about plugins, and an install compiles an
/// artifact. Holding it across that read is a window in which the whole interface waits for one package; cloning the
/// handle costs an `Arc` and releases the lock before any work starts.
///
/// # Errors
///
/// A sentence naming the stage that refused, because the caller sends it back to the peer as the sync result.
///
/// ★ `tauri-shell` only, because its only caller is the listener, which is the shell's.
#[cfg(feature = "tauri-shell")]
pub(crate) fn install_package(
    state: &AppState,
    db: &mut synra_store::SynraDb,
    archive: &std::path::Path,
) -> Result<synra_core::install::Installed, String> {
    let backend = state.plugins.as_ref().map(|set| set.lock().backend().clone());
    let Some(backend) = backend else {
        return Err("this shell has no plugin engine, so it cannot install a package".to_owned());
    };
    synra_core::install::from_package(
        db,
        &backend,
        &state.paths.plugins_dir(),
        archive,
        synra_store::sqlite::now_ms(),
        synra_plugin_host::verify::HostFacts::desktop(),
    )
    .map_err(|refusal| format!("{}: {}", refusal.stage, refusal.reason))
}

/// Asks the registry whether a newer version is published for an installed plugin.
///
/// # Errors
///
/// `PluginNotInstalled` when no active version exists, `Unsupported` when no registry is configured, and a fetch refusal's
/// own code for a network problem.
pub fn plugins_check_update(
    state: &AppState,
    plugin_id: &str,
) -> Result<Option<UpdateInfo>, IpcError> {
    crate::commands::registry::check_update(state, plugin_id)
}

/// Searches the configured registry for plugins.
///
/// The catalogue is a search over the registry's own index, scoped to packages that declare the plugin keyword; the
/// preview still shows the manifest's permissions before an install.
///
/// # Errors
///
/// `Unsupported` when no registry is configured or it serves no search endpoint, and a fetch refusal's own code for a
/// network problem.
pub fn plugins_catalog_search(
    state: &AppState,
    query: &CatalogQuery,
) -> Result<Vec<CatalogItem>, IpcError> {
    crate::commands::registry::search(state, query)
}

/// Describes a registry package before any of it is downloaded.
///
/// ★ A preview is the publisher's claim, read from the packument's inline manifest; the signature check happens on the
/// bytes. It is a question — "these are the permissions, do you want to fetch it?" — and never an approval.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or does not
/// publish its manifest inline, and a fetch refusal's own code for a network problem.
pub fn plugins_preview(
    state: &AppState,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<InstallPreviewDto, IpcError> {
    crate::commands::registry::preview(state, plugin_id, version)
}

/// Replaces an installed plugin with the newest version the registry publishes.
///
/// # Errors
///
/// `PluginNotInstalled` when no active version exists, `Unsupported` when no registry is configured, and the install
/// pipeline's refusals when the package cannot be fetched or verified.
pub fn plugins_update(state: &AppState, plugin_id: &str) -> Result<(), IpcError> {
    crate::commands::registry::update(state, plugin_id)
}

/// Installs a plugin from a directory or the configured registry, through the checks, and reports what the user has to be
/// told.
///
/// # What an install is not
///
/// It is not an approval. The row this writes is installed, switched off and untrusted, and the answer says `enabled:
/// false` so that a caller cannot read the absence of a decision as a decision.
///
/// # Errors
///
/// * `Unsupported` when this shell has no plugin engine at all, or no registry is configured for a registry source.
/// * `InvalidArgument` when a check refused the package; the params carry the stage and the reason.
pub fn plugins_install(state: &AppState, source: &InstallSourceDto) -> Result<InstallReport, IpcError> {
    match source {
        InstallSourceDto::Path { path } => install_directory(state, std::path::Path::new(path)),
        InstallSourceDto::Registry { .. } => crate::commands::registry::install_from_source(state, source),
        // A source a newer build added is not one this build can fetch; refusing it by name is the same answer the
        // variant got before there was a fetcher.
        _ => Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({"why": "this build does not know that install source"}))),
    }
}

/// Installs a package from a directory already on disk.
///
/// # Errors
///
/// * `Unsupported` when this shell has no plugin engine at all.
/// * `InvalidArgument` when a check refused the package; the params carry the stage and the reason.
pub(crate) fn install_directory(
    state: &AppState,
    path: &std::path::Path,
) -> Result<InstallReport, IpcError> {
    let installed = run_install(state, path, false)?;
    Ok(InstallReport {
        plugin_id: parse(&installed.plugin_id)?,
        version: installed.version,
        warnings: installed.warnings,
        enabled: false,
        publisher_id: installed.signed_by.as_ref().map(|publisher| publisher.publisher_id.clone()),
        fingerprint: installed.signed_by.as_ref().map(|publisher| publisher.fingerprint.clone()),
    })
}

/// Replaces the installed version of a plugin with the package at `path`.
///
/// # Errors
///
/// As [`install_directory`], plus `update` when nothing is installed at that identifier.
pub(crate) fn update_directory(state: &AppState, path: &std::path::Path) -> Result<(), IpcError> {
    run_install(state, path, true).map(|_| ())
}

/// The pipeline both install and update run: verify, stage, move, record.
fn run_install(
    state: &AppState,
    path: &std::path::Path,
    replace: bool,
) -> Result<synra_core::install::Installed, IpcError> {
    let plugins = state.plugin_set()?;
    let set = plugins.lock();
    // The database guard is scoped to the call rather than held while the report is built: this mutex is not reentrant,
    // and a `plugins_list` inside the same breadth would be a hang rather than an error.
    let outcome = {
        let mut db = state.db();
        let now = synra_store::sqlite::now_ms();
        let facts = synra_plugin_host::verify::HostFacts::desktop();
        if replace {
            synra_core::install::update_from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            )
        }
        else {
            synra_core::install::from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            )
        }
    };
    let installed = outcome.map_err(|refusal| {
        IpcError::unknown(refusal.code()).with_params(serde_json::json!({
            "stage": refusal.stage,
            "reason": refusal.reason,
        }))
    })?;
    // ★ The set's lock is released before anything reads it again.
    //
    // `announce` asks the set what is loaded, and this mutex is not reentrant: holding it across the call was a
    // same-thread self-deadlock, and the symptom was a test that hung rather than one that failed.
    drop(set);
    announce(state, &parse(&installed.plugin_id)?);
    Ok(installed)
}

/// Removes a plugin, and its data when the caller says so.
///
/// # The order, and what a failure in the middle leaves
///
/// The **record** goes first. A directory removed while its row remains is a plugin the list still offers that the
/// loader then reports as broken; the other order leaves files `record_discovered_plugins` would rediscover on the next
/// start, which at least shows the user something true. So the row goes first and the directory second, and a failure of
/// the second refuses the command rather than being swallowed — the user has to know the uninstall is half done.
///
/// # Why there is no event
///
/// Because the only plugin event the interface knows is `PluginUpdated`, which carries a card — and there is no card for
/// a plugin that no longer exists. The page reloads its list after this command, which is the honest shape for a
/// removal.
///
/// # Errors
///
/// `PluginNotInstalled` when no row has that identifier, and `Internal` when the store or the filesystem refuses.
pub fn plugins_uninstall(state: &AppState, plugin_id: &str, remove_data: bool) -> Result<(), IpcError> {
    let id = parse(plugin_id)?;
    let removed = state.db().remove_plugin(&id.to_string()).map_err(internal)?;
    if !removed {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id})));
    }
    synra_store::PluginFiles::new(state.paths.clone())
        .remove(&id.to_string(), remove_data)
        .map_err(internal)?;
    Ok(())
}


/// The plugins this device has, as cards.
///
/// # Errors
///
/// `Internal` when the table or a row cannot be read: an empty list would say "nothing is installed", which is a
/// different and false answer.
pub fn plugins_list(state: &AppState) -> Result<Vec<PluginSnapshot>, IpcError> {
    let rows = state.db().plugins().map_err(internal)?;

    // ★ One lock for the whole list.
    //
    // Taking it once per row would make a twenty-plugin list take the lock twenty times, and a plugin that compiles
    // slowly in between would make the list visibly uneven. One snapshot of the set, then every row answers against the
    // same snapshot — so the list is a moment in time rather than twenty.
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());

    let mut snapshots = Vec::with_capacity(rows.len());
    for row in rows {
        let plugin_id = PluginId::try_from(row.plugin_id.clone()).map_err(|error| {
            IpcError::unknown(synra_protocol::ErrorCode::Internal)
                .with_params(serde_json::json!({"pluginId": row.plugin_id, "detail": error.to_string()}))
        })?;
        snapshots.push(snapshot(state, &row, &plugin_id, loaded.as_deref()));
    }
    Ok(snapshots)
}

/// Enables a plugin.
///
/// # Errors
///
/// * [`IpcError::NotFound`] when no plugin of that identifier is installed.
/// * [`IpcError::Internal`] when the change cannot be written.
pub fn plugins_enable(state: &AppState, plugin_id: &str) -> Result<(), IpcError> {
    set_enabled(state, plugin_id, true)
}

/// Disables a plugin.
///
/// # Errors
///
/// As [`plugins_enable`].
pub fn plugins_disable(state: &AppState, plugin_id: &str) -> Result<(), IpcError> {
    set_enabled(state, plugin_id, false)
}

/// Writes the enabled flag.
///
/// # Errors
///
/// [`IpcError::NotFound`] when the plugin is not installed. Read back after the write rather than assumed, because an
/// `UPDATE` that matches no row reports success — and a caller that believed it would show the user a plugin that is
/// still off.
fn set_enabled(state: &AppState, plugin_id: &str, enabled: bool) -> Result<(), IpcError> {
    let id = parse(plugin_id)?;
    if !state.db().set_plugin_enabled(&id.to_string(), enabled, state.started_at_ms).map_err(internal)? {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id})),
        );
    }
    // ★ The interface learns about it here rather than on its next snapshot. The frontend has had an event dispatcher
    // since before there was anything to dispatch; a switch that flipped in the store and nowhere else is what that
    // gap looked like from the window.
    announce(state, &id);
    Ok(())
}

/// Tells the interface that a plugin's card changed, when it can be read back.
///
/// Failure to read the card is not a failure of the command: the write already happened, and refusing the command
/// afterwards would tell the user their change did not take effect when it did — the same reasoning as an audit write
/// that fails after the call it describes.
fn announce(state: &AppState, plugin_id: &PluginId) {
    let Ok(rows) = state.db().plugins() else {
        return;
    };
    let Some(row) = rows.into_iter().find(|row| row.plugin_id == plugin_id.to_string()) else {
        return;
    };
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    let card = snapshot(state, &row, plugin_id, loaded.as_deref());
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(card)));
}

/// Builds one card.
///
/// # Errors
///
/// Currently never. A plugin with no readable manifest falls back to its identifier for a name and reports no version,
/// because a manifest that will not parse is something the install path should have refused and the interface needs *a*
/// card either way — a user with a broken plugin needs to see it in order to remove it. The parse failure is not silently
/// swallowed though: it is what puts the card in the `unreadable` state below.
fn snapshot(
    state: &AppState,
    row: &synra_store::PluginRow,
    plugin_id: &PluginId,
    loaded: Option<&[PluginId]>,
) -> PluginSnapshot {
    let manifest = read_manifest(state, &row.plugin_id);
    let is_loaded = loaded.is_some_and(|ids| ids.contains(plugin_id));
    let decisions = state.db().plugin_grants(&row.plugin_id).unwrap_or_default();

    PluginSnapshot {
        plugin_id: plugin_id.clone(),
        title: manifest.as_ref().map_or_else(|| row.plugin_id.clone(), |manifest| manifest.title.clone()),
        description: manifest.as_ref().and_then(|manifest| manifest.description.clone()),
        version: manifest.as_ref().map_or_else(String::new, |manifest| manifest.version.clone()),
        publisher: manifest.as_ref().map_or_else(String::new, |manifest| manifest.publisher.id.clone()),
        enabled: row.enabled,
        state: life_state(row, is_loaded, manifest.is_some()).to_owned(),
        hybrid: manifest.as_ref().is_some_and(|manifest| {
            manifest.runtime.headless.is_some() && manifest.runtime.ui.is_some()
        }),
        // ★ Read from the manifest's page entry rather than from `hybrid`: a UI-only plugin has a page and no headless
        // half, and `plugin_ui_open` can open it — the card has to know that before it draws the button.
        has_ui: manifest.as_ref().is_some_and(|manifest| manifest.runtime.ui.is_some()),
        // ★ `None` rather than a zeroed reading.
        //
        // The host has not observed this plugin yet, so it has no usage to report. A snapshot full of zeros would be
        // indistinguishable from a plugin that genuinely never uses anything — and those two need different reactions
        // from a user deciding whether to keep it.
        health: None,
        // ★ What the plugin **asks for**, so the consent prompt has something to ask about. `granted` and `denied` are
        // both empty before the user answers, and a prompt that had to derive its list from them would have nothing to
        // show — which is why this field exists rather than the interface reading the manifest itself.
        requested: manifest
            .as_ref()
            .map(|manifest| manifest.capabilities.iter().map(|declared| declared.cap.clone()).collect())
            .unwrap_or_default(),
        // ★ Granted and denied are both listed, and both are sorted.
        //
        // Listing only the granted ones makes a refusal invisible, which is how a user ends up re-granting something
        // they deliberately refused — and being unable to see what they refused is what makes them do it.
        granted: decisions.iter().filter(|d| d.granted).map(|d| d.capability.clone()).collect(),
        denied: decisions.iter().filter(|d| !d.granted).map(|d| d.capability.clone()).collect(),
        // ★ `None` rather than a check performed here.
        //
        // Checking for an update is a network operation, and a list that made one would block the settings screen on
        // every open. The design document has a separate command for it; this one answers from what is on disk.
        update: None,
    }
}

/// Where a plugin is in its life.
///
/// Written as a single function over all four facts rather than derived by the interface, because the interface would
/// then have its own copy of the rule and the two would disagree the first time a case appeared that neither had been
/// written for.
fn life_state(row: &synra_store::PluginRow, loaded: bool, manifest_readable: bool) -> &'static str {
    if !row.enabled {
        return "disabled";
    }
    if !row.trusted {
        // Enabled but not trusted is the state that most needs a name of its own: the plugin is switched on, the user
        // believes it is running, and it is not — because nobody answered the consent prompt.
        return "awaiting_consent";
    }
    if !manifest_readable {
        return "unreadable";
    }
    if loaded {
        return "running";
    }
    // Enabled, trusted, readable, and not loaded. `unavailable` rather than `broken`: the shell may simply have no engine,
    // which is a property of this build rather than of the plugin.
    "unavailable"
}

/// Reads a plugin's manifest off disk.
///
/// Returns `None` rather than a refusal, because one unreadable manifest among twenty installed plugins must not stop the
/// other nineteen from being listed.
pub(crate) fn read_manifest(state: &AppState, plugin_id: &str) -> Option<Manifest> {
    let text = read_manifest_text(state, plugin_id)?;
    serde_json::from_str(&text).ok()
}

/// The manifest's bytes, exactly as they are on disk.
///
/// Separate from [`read_manifest`] because a consent decision is recorded against the **document** the user agreed to,
/// and a parsed manifest cannot be hashed back to the bytes it came from — field order and whitespace are gone by then,
/// and two files that differ only in those would hash alike.
fn read_manifest_text(state: &AppState, plugin_id: &str) -> Option<String> {
    let path = state.paths.plugin_dir(plugin_id).join("manifest.json");
    std::fs::read_to_string(path).ok()
}

/// Parses a plugin identifier the interface sent.
///
/// # Errors
///
/// [`IpcError::InvalidArgument`].
pub(crate) fn parse(plugin_id: &str) -> Result<PluginId, IpcError> {
    PluginId::try_from(plugin_id.to_owned()).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })
}

/// What a plugin tried to do, most recent last.
///
/// # ★ Why this command was waiting, and why it no longer is
///
/// The list carried it with the reason *"the audit ring has no production writer yet"*. That was true when it was
/// written: `AuditRing` was bounded, tested and queryable, and **nothing wrote to it** — the kernel's own
/// `record_action` had no host that constructs a kernel, and every plugin instance ran with no session, so no call was
/// ever authorised and none was ever recorded.
///
/// The writer exists now: a `PluginSet` installs a session on every instance it creates, drains what the session
/// recorded into its ring, and `audit_for` reads it back. So this is a read of a real trail rather than of an empty one.
///
/// # Why the last `limit` rather than the first
///
/// Because a settings screen shows what a plugin has been doing **lately**, and the interesting end of a trail is the
/// recent one. `limit` of zero returns nothing, which is what a caller asking for nothing gets.
///
/// # Errors
///
/// [`IpcError`] when this build has no plugin engine, which is a different answer from "this plugin has done nothing".
#[allow(clippy::result_large_err)]
pub fn plugins_audit(
    state: &AppState,
    plugin_id: &PluginId,
    limit: u32,
) -> Result<Vec<AuditEntryDto>, IpcError> {
    let set = state.plugin_set()?;
    let entries = set.lock().audit_for(plugin_id);
    // Taken from the end: `audit_for` answers oldest first, and a caller asking for `limit` lines wants the tail.
    let take = usize::try_from(limit).unwrap_or(usize::MAX);
    let start = entries.len().saturating_sub(take);
    // ★ `get` rather than indexing: `start` is `len - take`, which cannot exceed the length, but an index expression is a
    // panic waiting for the day that reasoning stops holding — and this workspace denies `indexing_slicing` so the compiler
    // asks for the safer spelling.
    Ok(entries.get(start..).unwrap_or_default().iter().map(entry_card).collect())
}

/// One recorded call, as the interface shows it.
///
/// ★ The DTO has a `trace_id` and the record does not.
///
/// `AuditEntry` carries when, who, against which capability and how it ended — and no trace, because the ring is
/// written by the session that made the call and the session is not given one. So this is always `None`, and it is
/// `None` because there is nothing to put there rather than because the lookup failed. Recorded here rather than
/// invented, and it is a gap in the *record*, not in this mapping.
fn entry_card(entry: &synra_observe::audit::AuditEntry) -> AuditEntryDto {
    AuditEntryDto {
        at_ms: entry.at_ms,
        capability: entry.capability.to_string(),
        allowed: entry.outcome.was_allowed(),
        // The outcome's stable name, for a refusal only. An allowed call has no reason to give, and a sentence written
        // here would be the shell describing the kernel's own decision.
        reason: entry.outcome.is_refusal().then(|| entry.outcome.as_str().to_owned()),
        elapsed_ms: entry.elapsed_ms,
        // See the note above: the record has no trace.
        trace_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        entry_card, plugins_audit, plugins_catalog_search, plugins_check_update, plugins_consent_respond,
        plugins_install, plugins_list, plugins_preview, plugins_publisher_decide, plugins_publishers,
    };
    use synra_ipc::{CatalogQuery, InstallSourceDto};
    use synra_protocol::PluginId;
    use crate::testing;

    const ID: &str = "@synra-plugin/chat";

    /// A state with a scripted engine, because installing needs one and the fixture builds a shell without.
    ///
    /// The backend answers from a table rather than compiling Wasm: what is being tested is the shell's command — the
    /// source it accepts, the report it returns and the row that lands — and the pipeline behind it has its own tests.
    fn with_engine(state: crate::state::AppState) -> crate::state::AppState {
        let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::with_platform(
            std::sync::Arc::clone(&state.db),
            std::sync::Arc::clone(&state.platform),
        ));
        let backend =
            synra_plugin_host::Backend::new(std::sync::Arc::new(synra_plugin_host::ScriptedBackend::new()));
        state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(
            synra_core::dispatch::PluginSet::new(backend, host),
        )))
    }

    /// A package directory a local install will accept: a manifest and a component.
    fn package(root: &std::path::Path) -> std::path::PathBuf {
        let directory = root.join("package");
        std::fs::create_dir_all(&directory).expect("a package directory");
        let manifest = format!(
            r#"{{"id": "@acme/hello", "version": "1.0.0", "title": "Hello",
                "publisher": {{"id": "@acme"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
            synra_plugin_host::host_world()
        );
        std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
        std::fs::write(directory.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("an artifact");
        directory
    }

    /// A `.tgz` holding these entries, written in memory.
    fn tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).expect("an entry");
        }
        builder.into_inner().expect("the encoder").finish().expect("a gzip tail")
    }

    #[test]
    fn a_registry_install_downloads_and_opens_the_tarball() {
        // ★ The whole first path: packument → tarball → extraction → the existing install pipeline. The server is a local
        // listener, so the test proves the client rather than a network.
        use std::io::{BufRead, BufReader, Write as _};

        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
        let manifest_json: serde_json::Value = serde_json::from_slice(&manifest).expect("a manifest document");

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
        let address = listener.local_addr().expect("an address");
        let base = format!("http://{address}");
        let document = serde_json::json!({
            "name": "@acme/hello",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@acme/hello",
                    "version": "1.0.0",
                    "dist": { "tarball": format!("{base}/pkg.tgz") },
                    "synra": manifest_json
                }
            }
        })
        .to_string();
        let archive = tgz(&[
            ("package/manifest.json", manifest.as_slice()),
            ("package/plugin.wasm", b"\0asm\x01\0\0\0"),
        ]);
        std::thread::spawn(move || {
            for _ in 0..3 {
                let Ok(mut stream) = listener.accept().map(|(stream, _)| stream) else { return };
                let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let body: &[u8] = if first.contains("pkg.tgz") {
                    &archive
                }
                else {
                    document.as_bytes()
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        });
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!(base), 1_000)
            .expect("the registry is configured");

        // ★ The preview is read before anything is downloaded, from the packument's inline manifest. It shows what the
        // package asks for; the install that follows still verifies the bytes and records the plugin switched off.
        let preview = plugins_preview(&state, "@acme/hello", None).expect("a preview");
        assert_eq!(preview.plugin_id.as_str(), "@acme/hello");
        assert_eq!(preview.version, "1.0.0");
        assert_eq!(preview.capabilities, ["log:*"]);
        assert!(preview.headless && !preview.has_ui);

        let report = plugins_install(
            &state,
            &InstallSourceDto::Registry {
                plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
                version: None,
            },
        )
        .expect("the registry package installs");

        assert_eq!(report.plugin_id.as_str(), "@acme/hello");
        assert_eq!(report.version, "1.0.0");
        assert!(state.db().plugins().expect("rows").iter().any(|row| row.plugin_id == "@acme/hello"));
        // The cache directory the tarball was opened in is gone, not left for the next run to find.
        let cache = state.paths.cache_dir().join("registry");
        assert!(
            !cache.read_dir().map(|entries| entries.count() > 0).unwrap_or(false),
            "the extracted package was left in the cache"
        );
    }

    #[test]
    fn installing_a_directory_records_it_switched_off() {
        // ★ The command an interface needs before it can offer an install at all. What it asserts is the answer and the
        // row: an install that reported success without writing one would look identical from the caller's side.
        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());

        let report = plugins_install(
            &state,
            &InstallSourceDto::Path { path: source.to_string_lossy().into_owned() },
        )
        .expect("it installs");

        assert_eq!(report.plugin_id.as_str(), "@acme/hello");
        assert_eq!(report.version, "1.0.0");
        assert!(!report.enabled, "installing is not approving");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let row = state
            .db()
            .plugins()
            .expect("the rows")
            .into_iter()
            .find(|row| row.plugin_id == "@acme/hello")
            .expect("a row");
        assert!(!row.enabled, "installing must not switch it on");
        assert!(!row.trusted, "installing must not trust it");
    }

    #[test]
    fn recording_consent_tells_the_interface() {
        // ★ The consent command published an event since it was wired, and nothing asserted it. The enable path has a
        // test; this one is the other half — a decision recorded from the interface has to reach the interface, or the
        // card it returns and the list another window draws disagree about whether the user approved anything.
        let (_root, state) = installed();
        let subscriber = state.events.subscribe();

        plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

        let delivery = state.events.take(subscriber).expect("one event");
        assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
        match delivery.events().first() {
            Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
                assert_eq!(card.plugin_id.as_str(), ID);
                assert!(!card.granted.is_empty(), "the card carries the decision that was just recorded");
            }
            other => panic!("expected a plugin card, got {other:?}"),
        }
    }
    #[test]
    fn enabling_a_plugin_tells_the_interface() {
        // ★ The producer the event dispatcher never had. A switch that flipped in the store and nowhere else is what
        // "the shell never emits" looked like from the window: the page showed whatever it had read when it opened.
        let (_root, state) = installed();
        let subscriber = state.events.subscribe();

        super::plugins_enable(&state, ID).expect("the plugin is enabled");

        let delivery = state.events.take(subscriber).expect("one event");
        assert_eq!(delivery.first_seq(), 1, "the bus numbers it, not this command");
        match delivery.events().first() {
            Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
                assert_eq!(card.plugin_id.as_str(), ID);
                assert!(card.enabled, "the card says what the row now says");
            }
            other => panic!("expected a plugin card, got {other:?}"),
        }
    }

    #[test]
    fn a_registry_install_without_a_configured_registry_is_refused() {
        // ★ Refused by the missing configuration, not by a network failure: there is nowhere to dial, and "could not reach
        // the registry" would send a user to check a connection that was never used.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);

        let refused = plugins_install(
            &state,
            &InstallSourceDto::Registry {
                plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
                version: None,
            },
        )
        .expect_err("no registry is configured");

        assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    }

    #[test]
    fn a_catalog_search_lists_what_the_registry_index_returns() {
        // ★ Results are filtered to plugin ids: an npm index scoped by keyword can still return a package whose name is
        // not one this device could install, and showing it would offer a button that cannot work.
        use std::io::{BufRead, BufReader, Write as _};

        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        plugins_install(
            &state,
            &InstallSourceDto::Path { path: source.to_string_lossy().into_owned() },
        )
        .expect("the fixture installs");

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
        let address = listener.local_addr().expect("an address");
        let base = format!("http://{address}");
        let body = r#"{"objects":[
            {"package":{"name":"@acme/zeta","version":"2.0.0","publisher":{"username":"acme"}}},
            {"package":{"name":"@acme/hello","version":"1.0.0","description":"Hello","publisher":{"username":"acme"}}},
            {"package":{"name":"not a plugin","version":"1.0.0"}}
        ]}"#;
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            // ★ The page is the parameter npm spells `from`; a client that forgot it would ask for page one forever.
            assert!(first.contains("from=0"), "the offset was not sent: {first}");
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        });
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!(base), 1_000)
            .expect("the registry is configured");

        let found = plugins_catalog_search(
            &state,
            &CatalogQuery {
                query: "hello".to_owned(),
                registry: None,
                limit: Some(10),
                offset: Some(0),
                sort: Some(synra_ipc::dto::CatalogSort::Name),
            },
        )
        .expect("a search");

        assert_eq!(found.len(), 2, "a name that is not a plugin id is filtered out");
        assert_eq!(found[0].plugin_id.as_str(), "@acme/hello", "name sort puts hello before zeta");
        assert_eq!(found[1].plugin_id.as_str(), "@acme/zeta");
        assert_eq!(found[0].version, "1.0.0");
        assert_eq!(found[0].publisher, "acme");
        assert!(found[0].installed, "the fixture is installed");
    }

    #[test]
    fn an_update_check_names_the_version_and_the_new_permission() {
        // ★ The comparison is against the installed manifest on disk, not against a row: the capabilities a user approved
        // are the ones in the package they have, and the store does not keep them.
        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        plugins_install(
            &state,
            &InstallSourceDto::Path { path: source.to_string_lossy().into_owned() },
        )
        .expect("the fixture installs");

        let manifest = serde_json::json!({
            "id": "@acme/hello",
            "version": "2.0.0",
            "title": "Hello",
            "publisher": {"id": "@acme"},
            "sdk": {"min": "0.1.0"},
            "wit": synra_plugin_host::host_world(),
            "capabilities": [{"cap": "log:*"}, {"cap": "device:send:*"}],
            "runtime": {"headless": {"artifact": "plugin.wasm", "stateful": false}}
        });
        let document = serde_json::json!({
            "name": "@acme/hello",
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "2.0.0": {
                    "name": "@acme/hello",
                    "version": "2.0.0",
                    "dist": { "tarball": "https://registry.example.com/hello-2.0.0.tgz" },
                    "synra": manifest
                }
            }
        })
        .to_string();
        let (base, _served) = serve(move |_| (200, document.clone().into_bytes()));
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!(base), 1_000)
            .expect("the registry is configured");

        let update = plugins_check_update(&state, "@acme/hello")
            .expect("the check runs")
            .expect("a newer version exists");

        assert_eq!(update.version, "2.0.0");
        assert!(update.needs_new_consent, "the new manifest asks for a device permission");
        assert_eq!(update.added_capabilities, ["device:send:*"]);
        // The installed version is not in the document, so the same check against itself cannot be exercised here; the
        // equal-version branch is a string comparison and fails closed (no update) rather than inventing one.
    }

    /// A tiny HTTP server for the registry tests: answers each request from `handler`, and counts what it served.
    fn serve(
        handler: impl Fn(&str) -> (u16, Vec<u8>) + Send + 'static,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{BufRead, BufReader, Write as _};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
        let address = listener.local_addr().expect("an address");
        let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&served);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let _ = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let (status, body) = handler(&first);
                let reason = if status == 200 { "OK" } else { "Not Found" };
                let head = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (format!("http://{address}"), served)
    }

    #[test]
    fn a_mirror_that_fails_is_followed_by_the_next_one() {
        // ★ The first mirror serves the packument and then refuses its own tarball; the second serves the same path. The
        // install must succeed, and the second mirror must have been asked — a client that retried the first one only
        // would pass a test that only counted the install.
        use std::sync::{Arc, Mutex};

        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
        let archive = tgz(&[
            ("package/manifest.json", manifest.as_slice()),
            ("package/plugin.wasm", b"\0asm\x01\0\0\0"),
        ]);

        let (second, second_count) = serve(move |line: &str| {
            if line.contains("pkg.tgz") {
                (200, archive.clone())
            }
            else {
                (404, Vec::new())
            }
        });
        let document = Arc::new(Mutex::new(String::new()));
        let served_document = Arc::clone(&document);
        let (first, first_count) = serve(move |line: &str| {
            if line.contains("%2F") {
                (200, served_document.lock().expect("the document").clone().into_bytes())
            }
            else {
                (404, Vec::new())
            }
        });
        *document.lock().expect("the document") = serde_json::json!({
            "name": "@acme/hello",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@acme/hello",
                    "version": "1.0.0",
                    // On the first mirror, whose `/pkg.tgz` is a 404; the second mirror serves the same path.
                    "dist": { "tarball": format!("{first}/pkg.tgz") }
                }
            }
        })
        .to_string();

        state
            .db()
            .set_config("plugins.registry", &serde_json::json!([first, second]), 1_000)
            .expect("the mirrors are configured");

        let report = plugins_install(
            &state,
            &InstallSourceDto::Registry {
                plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
                version: None,
            },
        )
        .expect("the second mirror completes the install");

        assert_eq!(report.version, "1.0.0");
        assert!(first_count.load(std::sync::atomic::Ordering::Relaxed) >= 1, "the first mirror was not asked");
        assert!(
            second_count.load(std::sync::atomic::Ordering::Relaxed) >= 1,
            "the second mirror was never asked"
        );
    }

    /// A manifest declaring the two capabilities the consent tests decide about.
    ///
    /// The shape is the one `synra-plugin-api`'s own fixture uses, because a hand-written manifest that drifted from it
    /// would test the parser rather than this command.
    const MANIFEST: &str = r#"{
        "id": "@synra-plugin/chat",
        "version": "1.0.0",
        "title": "Chat",
        "publisher": { "id": "synra-official" },
        "sdk": { "min": "1.0.0" },
        "wit": "synra:plugin@1.0.0",
        "runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } },
        "capabilities": [{ "cap": "log:*" }, { "cap": "device:list" }]
    }"#;

    /// A state with the plugin installed on disk and in the database, awaiting consent.
    fn installed() -> (testing::Root, crate::state::AppState) {
        let (root, state) = testing::file_state();
        let directory = state.paths.plugin_dir(ID);
        std::fs::create_dir_all(&directory).expect("the plugin directory");
        std::fs::write(directory.join("manifest.json"), MANIFEST).expect("a manifest");
        state
            .db()
            .insert_plugin(ID, true, false, "installed", testing::FIXED_NOW_MS, testing::FIXED_NOW_MS)
            .expect("a plugin row");
        (root, state)
    }

    fn grants(state: &crate::state::AppState) -> Vec<(String, bool)> {
        state
            .db()
            .plugin_grants(ID)
            .expect("the decisions")
            .into_iter()
            .map(|row| (row.capability, row.granted))
            .collect()
    }

    #[test]
    fn a_static_index_installs_the_tarball_it_names() {
        // ★ A bucket has no packument endpoint: one JSON document per package names the versions and their tarballs, and
        // the install path is the same one an npm registry feeds.
        use std::sync::{Arc, Mutex};

        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        let manifest = std::fs::read(source.join("manifest.json")).expect("the manifest");
        let manifest_json: serde_json::Value = serde_json::from_slice(&manifest).expect("a manifest document");
        let archive = tgz(&[
            ("package/manifest.json", manifest.as_slice()),
            ("package/plugin.wasm", b"\0asm\x01\0\0\0"),
        ]);

        let document = Arc::new(Mutex::new(String::new()));
        let served = Arc::clone(&document);
        let (base, _served) = serve(move |line: &str| {
            if line.contains("hello.json") {
                (200, served.lock().expect("the document").clone().into_bytes())
            }
            else if line.contains("pkg.tgz") {
                (200, archive.clone())
            }
            else {
                (404, Vec::new())
            }
        });
        *document.lock().expect("the document") = serde_json::json!({
            "name": "@acme/hello",
            "latest": "1.0.0",
            "publishers": { "@acme": "AB12 CD34 EF56 7890" },
            "versions": [{
                "version": "1.0.0",
                "tarball": format!("{base}/pkg.tgz"),
                "manifest": manifest_json
            }]
        })
        .to_string();
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!(format!("static:{base}")), 1_000)
            .expect("the static index is configured");

        // ★ The directory's fingerprint is shown before the tarball is downloaded; the signature is still checked on the
        // bytes after it arrives.
        let preview = plugins_preview(&state, "@acme/hello", None).expect("a preview");
        assert_eq!(preview.publisher_key.as_deref(), Some("AB12 CD34 EF56 7890"));

        let report = plugins_install(
            &state,
            &InstallSourceDto::Registry {
                plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
                version: None,
            },
        )
        .expect("the static index installs");

        assert_eq!(report.plugin_id.as_str(), "@acme/hello");
        assert_eq!(report.version, "1.0.0");
    }

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
            &InstallSourceDto::Path { path: source.to_string_lossy().into_owned() },
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
                let Ok((mut stream, _)) = listener.accept() else { return };
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
                if headers.to_ascii_lowercase().contains("if-none-match: \"v1\"") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    );
                }
                else {
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
            .set_config("plugins.registry", &serde_json::json!(format!("static:{base}")), 1_000)
            .expect("the static index is configured");

        let first = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");
        let second = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");

        assert_eq!(first.version, "3.0.0");
        assert_eq!(second.version, "3.0.0", "the 304 answered from the cache, not from a second body");
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
            &InstallSourceDto::Path { path: source.to_string_lossy().into_owned() },
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
                let Ok((mut stream, _)) = listener.accept() else { return };
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
                }
                else {
                    let document = if seen == 0 { body("3.0.0") } else { body("4.0.0") };
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
            .set_config("plugins.registry", &serde_json::json!(format!("static:{base}")), 1_000)
            .expect("the static index is configured");

        let first = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");
        let second = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");

        assert_eq!(first.version, "3.0.0");
        assert_eq!(second.version, "4.0.0", "the expired document was read again");
        assert_eq!(requests.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_static_index_has_no_search_to_offer() {
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!("static:https://static.example.com"), 1_000)
            .expect("the static index is configured");

        let refused = plugins_catalog_search(
            &state,
            &CatalogQuery { query: "hello".to_owned(), registry: None, limit: None, offset: None, sort: None },
        )
        .expect_err("there is no index to search");

        assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported, "{refused:?}");
    }

    #[test]
    fn a_card_says_whether_the_plugin_has_a_page() {
        // ★ `has_ui` is not `hybrid`: a UI-only plugin has a page and no headless half, and `plugin_ui_open` can open
        // it. The card needs both facts separately, or the one shape with a page but no Wasm would have no way in.
        let (_root, state) = installed();

        let headless_only = plugins_list(&state).expect("the list").remove(0);
        assert!(!headless_only.has_ui, "a headless-only manifest has no page");
        assert!(!headless_only.hybrid);

        // The same plugin with a UI half written over its manifest. The entry is what `plugin_ui_open` resolves, so a
        // card that reported `has_ui` without one would draw a button that cannot work.
        let manifest = MANIFEST.replace(
            r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } }"#,
            r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false },
                "ui": { "artifact": "dist/ui", "entry": "index.html" } }"#,
        );
        std::fs::write(state.paths.plugin_dir(ID).join("manifest.json"), manifest).expect("a manifest with a page");

        let with_page = plugins_list(&state).expect("the list").remove(0);
        assert!(with_page.has_ui, "the page entry is not reported");
        assert!(with_page.hybrid, "this manifest has both halves");
    }

    #[test]
    fn an_audit_entry_is_mapped_from_the_record_rather_than_re_derived() {
        // ★ What this command is responsible for.
        //
        // The ring's own read and write are tested where they live: `synra-observe` bounds and queries it, and
        // `synra-core` drains a session's record into it. What is left here is the mapping, and the mapping is where a
        // wrong answer would look right — an allowed call reported with a reason, or a refusal reported as allowed.
        let allowed = entry_card(&synra_observe::audit::AuditEntry::new(
            1_700_000_000_000,
            PluginId::parse(ID).expect("a plugin id"),
            synra_capability::Capability::parse("log:info").expect("a capability"),
            synra_observe::audit::AuditOutcome::Allowed,
            Some(7),
        ));
        assert_eq!(allowed.capability, "log:info");
        assert!(allowed.allowed);
        assert_eq!(allowed.elapsed_ms, Some(7));
        // ★ An allowed call has no reason to give. A sentence here would be the shell describing the kernel's
        // own decision, and it would read as a complaint about a call that worked.
        assert_eq!(allowed.reason, None);

        let denied = entry_card(&synra_observe::audit::AuditEntry::new(
            1_700_000_000_001,
            PluginId::parse(ID).expect("a plugin id"),
            synra_capability::Capability::parse("device:list").expect("a capability"),
            synra_observe::audit::AuditOutcome::Denied,
            None,
        ));
        assert!(!denied.allowed);
        // The outcome's stable name, which the interface translates — not a sentence written here.
        assert_eq!(denied.reason.as_deref(), Some("denied"));
        // And always `None`: the record carries no trace, which is a gap in the *record* rather than in this mapping.
        assert_eq!(denied.trace_id, None);
    }

    #[test]
    fn without_an_engine_the_audit_command_says_so_rather_than_answering_with_nothing() {
        // ★ The rule every command in this module shares: it either answers or says why it cannot.
        //
        // "This plugin has done nothing" and "this build cannot run plugins, so there is no trail to read" look the same
        // on a screen — an empty list — and a user acts on them differently. The fixture has no engine, so this is the
        // case that can be asserted here; the empty-but-real trail is covered by the mapping test above, which drives an
        // entry through `entry_card` rather than through a ring this fixture cannot fill.
        let state = crate::testing::state();

        let refused = plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10)
            .expect_err("this fixture has no engine");

        assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    }

    #[test]
    fn granting_every_capability_makes_the_plugin_trusted_and_records_each_decision() {
        // ★ The act the whole table was built for, and the one nothing could perform: `plugin_grants` had a schema, an
        // index, a row type, a reader and **no writer at all**, so the loader took the manifest's own declaration as the
        // grant and a plugin received exactly what it asked for.
        let (_root, state) = installed();

        let card = plugins_consent_respond(
            &state,
            ID,
            &["log:*".to_owned(), "device:list".to_owned()],
        )
        .expect("consent");

        assert_ne!(card.state, "awaiting_consent", "the plugin is trusted now: {card:?}");
        assert_eq!(card.granted, vec!["device:list".to_owned(), "log:*".to_owned()]);
        assert!(card.denied.is_empty());
        assert_eq!(
            grants(&state),
            vec![("device:list".to_owned(), true), ("log:*".to_owned(), true)],
            "both decisions are on disk"
        );
    }

    #[test]
    fn declining_one_capability_records_the_denial_and_still_trusts() {
        // ★ Two claims in one test, and the second is the one this host used to get wrong.
        //
        // A declined capability is recorded as a **denial**, not omitted: a plugin whose manifest declares two
        // capabilities and whose table holds one row is one the interface cannot describe, so the user is shown a
        // capability they never answered about.
        //
        // And the plugin is **trusted anyway**: it may run with what it was allowed and be refused at the gate for the
        // rest. This host used to require every capability, the daemon required any, and the loader took the rows as
        // the authority — so the same decision produced a plugin that could not run here and one that ran there.
        let (_root, state) = installed();

        let card = plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

        assert_eq!(card.granted, vec!["log:*".to_owned()]);
        assert_eq!(card.denied, vec!["device:list".to_owned()]);
        assert_ne!(card.state, "awaiting_consent", "one allowed capability is approval");
        assert_eq!(
            grants(&state),
            vec![("device:list".to_owned(), false), ("log:*".to_owned(), true)],
        );
    }

    #[test]
    fn denying_every_capability_leaves_it_awaiting_consent() {
        // The other side of the rule: a decision that allows nothing is a refusal, and `should_run` is
        // `enabled && trusted` — so the row stays unable to run even though the user answered the prompt.
        let (_root, state) = installed();

        let card = plugins_consent_respond(&state, ID, &[]).expect("consent");

        assert_eq!(card.state, "awaiting_consent");
        assert!(card.granted.is_empty());
        assert_eq!(card.denied.len(), 2);
    }

    #[test]
    fn changing_a_decision_overwrites_it_rather_than_adding_a_second_answer() {
        // The table's primary key is `(plugin_id, capability)`, so a user who changes their mind must land on one row
        // per capability. Two rows for one capability would make "what did the user decide" unanswerable — and the
        // reader orders by capability, so the two would sit next to each other and look like two different facts.
        let (_root, state) = installed();

        plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("a first answer");
        assert_eq!(grants(&state).len(), 2);

        plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
            .expect("a changed answer");

        let decisions = grants(&state);
        assert_eq!(decisions.len(), 2, "still one row per capability: {decisions:?}");
        assert!(
            decisions.iter().all(|(_, granted)| *granted),
            "the second answer is the one that stands: {decisions:?}"
        );
    }

    #[test]
    fn a_plugin_whose_manifest_cannot_be_read_is_refused_rather_than_consented_to_blind() {
        // ★ Treating an unreadable manifest as "declares nothing" would record an empty decision set, which is
        // indistinguishable from a user who declined everything — and would mark the plugin trusted while nothing had
        // been shown to anybody.
        let (root, state) = testing::file_state();
        state
            .db()
            .insert_plugin(ID, true, false, "installed", testing::FIXED_NOW_MS, testing::FIXED_NOW_MS)
            .expect("a plugin row");
        drop(root);

        let refused = plugins_consent_respond(&state, ID, &[]);
        assert!(refused.is_err(), "a missing manifest is not a decision");
        assert!(grants(&state).is_empty(), "and nothing was recorded");
    }

    #[test]
    fn the_decision_is_recorded_against_the_manifest_it_was_made_about() {
        // ★ Why `manifest_digest` is a column. A consent decision is about a *document*, and a plugin that ships an
        // update declaring more capabilities is a different document — so the rows that predate it are visibly older
        // than the manifest on disk. That is what makes silent capability growth detectable instead of invisible.
        let (_root, state) = installed();
        plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
            .expect("consent");
        let before = state.db().plugin_grants(ID).expect("decisions");
        let digest_before = before.first().expect("a row").manifest_digest.clone();
        assert_eq!(digest_before.len(), 64, "sha256 in hex: {digest_before}");

        // The same manifest decided about twice hashes the same, so an unchanged plugin does not look like a changed one.
        plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
            .expect("consent again");
        let after = state.db().plugin_grants(ID).expect("decisions");
        assert!(
            after.iter().all(|row| row.manifest_digest == digest_before),
            "an unchanged manifest must not look changed: {after:?}"
        );
    }

    #[test]
    fn uninstalling_removes_the_row_and_keeps_the_data() {
        // ★ The default the design document asks for: a plugin the user may reinstall keeps its settings, and deleting
        // them unless asked would be the interface deciding what is replaceable.
        let (_root, state) = installed();
        let data = state.paths.plugin_sandbox_dir(ID);
        std::fs::create_dir_all(&data).expect("some plugin data");

        super::plugins_uninstall(&state, ID, false).expect("uninstalls");

        assert!(
            state.db().plugins().expect("rows").iter().all(|row| row.plugin_id != ID),
            "the row is gone"
        );
        assert!(!state.paths.plugin_dir(ID).exists(), "and the artifacts with it");
        assert!(data.exists(), "but the data stays unless asked for");
    }

    #[test]
    fn asking_for_the_data_removes_it_as_well() {
        // ★ The other half of the `remove_data` parameter, which the page never sends — and which nothing tested. It is the
        // one value of it that cannot be undone, so an untested `true` is the worst one to leave untested.
        let (_root, state) = installed();
        let data = state.paths.plugin_sandbox_dir(ID);
        std::fs::create_dir_all(&data).expect("some plugin data");

        super::plugins_uninstall(&state, ID, true).expect("uninstalls");

        assert!(!data.exists(), "the data went with the plugin");
    }

    #[test]
    fn uninstalling_something_that_is_not_installed_is_a_refusal() {
        // A typo and a no-op are different answers, and this one names the identifier so the user can see their own.
        let (_root, state) = installed();

        let refused = super::plugins_uninstall(&state, "@acme/typo", false).expect_err("nothing is there");

        assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
    }

    #[test]
    fn a_card_says_what_the_plugin_asks_for() {
        // ★ The field the consent prompt reads. `granted` and `denied` are both empty before the user answers, so a card
        // carrying only those could not say what to ask about — and the prompt would have to read the manifest itself,
        // which is the shell doing the kernel's job.
        let (_root, state) = installed();

        let cards = plugins_list(&state).expect("the list");

        let card = cards.iter().find(|card| card.plugin_id.as_str() == ID).expect("the card");
        assert_eq!(card.requested, vec!["log:*".to_owned(), "device:list".to_owned()]);
        assert!(card.granted.is_empty(), "nothing has been decided yet");
    }

    #[test]
    fn a_plugin_awaiting_consent_is_listed_and_says_so() {
        // The list is where the user finds the plugin to answer about, so the state the consent prompt is triggered by
        // has to be the state the list reports. Asserted through `plugins_list` rather than the consent command's own
        // answer, because the interface reaches this one first.
        let (_root, state) = installed();

        let listed = plugins_list(&state).expect("a list");
        let card = listed.first().expect("the installed plugin");
        assert_eq!(card.plugin_id.as_str(), ID);
        assert_eq!(card.state, "awaiting_consent");
        assert!(card.granted.is_empty() && card.denied.is_empty(), "nothing has been decided yet");
    }

    #[test]
    fn a_publisher_decision_needs_a_key_this_device_has_met() {
        // ★ "Trusted" is a decision on a row that exists because a signature was verified. A command that could create
        // the row would let the interface trust a key no package ever carried.
        let state = crate::testing::state();

        let refused = plugins_publisher_decide(&state, "@ghost", "trusted").expect_err("nobody has met it");
        assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);

        state
            .db()
            .record_publisher("@acme", "AB12 CD34 EF56 7890", "a2V5", 1_000)
            .expect("the key was seen");
        plugins_publisher_decide(&state, "@acme", "trusted").expect("a known key can be trusted");
        let listed = plugins_publishers(&state).expect("a list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].trust, "trusted");

        let refused = plugins_publisher_decide(&state, "@acme", "maybe").expect_err("not a decision");
        assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
    }
}
