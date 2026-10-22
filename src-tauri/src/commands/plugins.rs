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
    PluginHealthDto, PluginSnapshot, RemoveReport, UpdateInfo,
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
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(card.clone())));
    Ok(card)
}

/// Records every plugin directory the database does not know about.
///
/// # Why the shell needs this at all
///
/// Because it has its own data directory. The daemon runs the same scan over *its* `plugins/`, so a directory an author
/// copied into the window's data directory was invisible in the window while the identical directory beside the daemon
/// appeared — two answers to "what is installed" on one machine. The scan records and does not trust: a row it writes is
/// disabled and untrusted, so this is visibility, not approval.
///
/// # Errors
///
/// None. A directory that is not a plugin is a `problem` in the returned list; the caller prints it, because the
/// difference between "nothing is installed" and "the installation is half-copied" is one a user has to be told.
#[cfg(any(feature = "tauri-shell", test))]
pub(crate) fn discover_plugins(
    state: &AppState,
    now_ms: i64,
) -> Vec<synra_core::plugin::Discovered> {
    let found = {
        let db = state.db();
        synra_core::plugin::record_discovered_plugins(&db, &state.paths.plugins_dir(), now_ms)
    };
    for discovered in &found {
        match &discovered.problem {
            Some(problem) => {
                eprintln!("synra-app: {} was not recorded as a plugin: {problem}", discovered.plugin_id);
            }
            None => eprintln!("synra-app: {} was found and recorded as installed, switched off", discovered.plugin_id),
        }
    }
    found
}

/// Loads every installed plugin the database says should run.
///
/// # Why this is a shell function and not a startup-only step
///
/// Because the same pass is what tells the interface why a plugin is not running. The summary it returns is stored on
/// [`AppState`](crate::AppState) and read by [`plugins_list`], so a card's `stateReason` and the set of plugins that can
/// actually answer are two views of one record.
///
/// # Errors
///
/// None. A shell with no engine has nothing to load, and a plugin that fails is a row in the summary — the same shape
/// every other failure in this loader takes, because one broken plugin must not stop the others.
#[cfg(any(feature = "tauri-shell", test))]
pub(crate) fn load_installed_plugins(
    state: &AppState,
    verdict: &synra_observe::crash::StartupVerdict,
    revocations: &synra_plugin_api::revocation::RevocationList,
) -> synra_core::plugin::LoadedSummary {
    let Ok(plugins) = state.plugin_set() else {
        // No engine: the caller records why on the state, and there is nothing to load. `plugins_list` explains it from
        // `engine_refusal` rather than from an empty summary.
        return synra_core::plugin::LoadedSummary::default();
    };
    let mut set = plugins.lock();
    // ★ The environment is read before the store lock. The loader is about to hold the store, and a host's `locale()`
    // may need it; asking from inside the loader was a same-thread deadlock on the daemon side.
    let activation = synra_core::plugin::activation_config(set.host().as_ref());
    let summary = {
        let db = state.db();
        synra_core::plugin::load_installed_with_config(
            &mut set,
            &db,
            &state.paths.plugins_dir(),
            verdict,
            revocations,
            &activation,
        )
    };
    // ★ The set's lock is released before anything reads it again. `plugins_list` takes the same lock, and this mutex is
    // not reentrant — holding it across the list would be a same-thread self-deadlock whose symptom is a hung window.
    drop(set);
    if let Some(why) = &summary.safe_mode {
        eprintln!("synra-app: starting without plugins: {why}");
    }
    for refused in &summary.refused {
        // Printed, because a plugin that failed to load is invisible otherwise and a silent skip is how a broken plugin
        // becomes a support question months later.
        eprintln!("synra-app: {} was not loaded: {}", refused.plugin_id, refused.reason);
    }
    summary
}

/// The revocation list shipped beside the data directory, or an empty one.
///
/// # Why a missing file is not an error
///
/// Because the list ships with the application and the ordinary install may not have one: "nothing is revoked" and
/// "there is no list" are different facts, but they lead to the same action here, and refusing to work over the second
/// would trade a working application for a diagnostic absence. A file that **is** there and cannot be read is reported,
/// because silently ignoring a revocation list is the one failure in this area a user would never forgive.
pub(crate) fn revocations(state: &AppState) -> synra_plugin_api::revocation::RevocationList {
    let path = state.paths.data_dir().join("revocations.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return synra_plugin_api::revocation::RevocationList::empty();
    };
    match synra_plugin_api::revocation::RevocationList::parse(&text) {
        Ok(list) => {
            if !list.is_empty() {
                eprintln!("synra: {} revocation entries read from {}", list.len(), path.display());
            }
            list
        }
        Err(error) => {
            eprintln!(
                "synra: {} could not be read as a revocation list ({error}); no plugin is disabled on its account",
                path.display()
            );
            synra_plugin_api::revocation::RevocationList::empty()
        }
    }
}

/// Brings one plugin's compiled form in line with the row the user just changed.
///
/// # Why a command calls this
///
/// Because start-up is not the only moment a plugin begins or stops running. Enabling after install, confirming the
/// capabilities of an already-enabled plugin, and updating all change the answer to "should this run", and a host that
/// only loaded at start-up would make all three require a restart. The design's lifecycle says enable is *activating*;
/// this is the function that makes that true without moving a WebView.
///
/// # Errors
///
/// The reason the engine or the manifest refused, so an enable can tell the user instead of leaving a switch on over a
/// plugin that is not running. The refusal is recorded on the load report first, so the card carries the same sentence
/// the command returns.
fn apply_lifecycle(state: &AppState, id: &PluginId) -> Result<(), String> {
    let Ok(plugins) = state.plugin_set() else {
        // No engine: the row is the user's decision and the card explains why nothing runs. A command that failed here
        // would make "the plugin is on but this build cannot run it" look like the switch did not take.
        return Ok(());
    };
    let should_run = {
        let db = state.db();
        let rows = db.plugins().map_err(|error| format!("the plugin rows could not be read: {error}"))?;
        // The database lock is released before the set is taken: the two locks are never held in one order here, so
        // there is no second order for another command to deadlock on.
        drop(db);
        rows.into_iter().find(|row| row.plugin_id == id.as_str()).is_some_and(|row| row.should_run())
    };
    if !should_run {
        apply_unload(state, id);
        return Ok(());
    }

    let revocations = revocations(state);
    // ★ Read before the store lock, for the reason in `load_installed_plugins`: a host's locale may reach the store.
    let activation = {
        let set = plugins.lock();
        synra_core::plugin::activation_config(set.host().as_ref())
    };
    let mut set = plugins.lock();
    // ★ A faulted plugin is retried as itself: clear the fault and call `activate` again. The old path unloaded and
    // reloaded it, which threw away the compiled artifact and the lifecycle record just to reach the same call. Only a
    // plugin the ladder actually faulted takes this path; every other reason to reload still reloads.
    let faulted = matches!(
        set.lifecycle(id).map(synra_plugin_host::Lifecycle::state),
        Some(synra_plugin_host::State::Faulted { .. })
    );
    if faulted && set.clear_fault(id, u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0)).unwrap_or(false) {
        let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0);
        let outcome = synra_core::plugin::reactivate_loaded(&mut set, id, &activation, now);
        drop(set);
        return match outcome {
            Ok(()) => {
                state.note_plugin_loaded(id.clone());
                Ok(())
            }
            Err(reason) => {
                plugins.lock().unload(id);
                Err(format!("it loaded but did not activate: {reason}"))
            }
        };
    }
    let outcome = {
        let db = state.db();
        synra_core::plugin::load_plugin_with_config(
            &mut set,
            &db,
            &state.paths.plugins_dir(),
            id.as_str(),
            &revocations,
            &activation,
        )
    };
    // ★ The set's lock is gone before the report is written. `announce` and `plugins_list` take the same lock, and this
    // mutex is not reentrant — a same-thread deadlock whose only symptom is a window that stops answering.
    drop(set);
    match outcome {
        Ok(loaded) => {
            state.note_plugin_loaded(loaded);
            Ok(())
        }
        Err(reason) => {
            state.note_plugin_refused(id.as_str(), reason.clone());
            Err(reason)
        }
    }
}

/// Drops one plugin's compiled form and its place in the load report.
fn apply_unload(state: &AppState, id: &PluginId) {
    if let Ok(plugins) = state.plugin_set() {
        // ★ `deactivate` first: it is the plugin's one chance to release what it holds, and the loader that started it
        // is what has to stop it. The set's lock is released by the guard leaving this block, before the report's lock.
        let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0);
        synra_core::plugin::deactivate(&mut plugins.lock(), id, now);
    }
    state.plugin_loads.lock().record_unloaded(id);
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

/// Tries one plugin's load again without touching its flags.
///
/// # Why this exists
///
/// A faulted plugin is installed, enabled and trusted; what failed was starting it. The only way to ask again was to
/// switch it off and on, which is a flag change the user did not want and which the page had to explain as a workaround.
/// This is the same `apply_lifecycle` an enable runs, on a row that is already where it should be.
///
/// # Errors
///
/// [`IpcError::PluginNotInstalled`] when no row exists, and `Internal` when the list cannot be read. A load that fails is
/// **not** an error: the answer is the card, and its `stateReason` is the sentence the loader produced.
pub fn plugins_reload(state: &AppState, plugin_id: &str) -> Result<PluginSnapshot, IpcError> {
    let id = parse(plugin_id)?;
    let row = state
        .db()
        .plugins()
        .map_err(internal)?
        .into_iter()
        .find(|row| row.plugin_id == id.as_str())
        .ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id}))
        })?;

    let _ = apply_lifecycle(state, &id);
    announce(state, &id);
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    snapshot(state, &row, &id, loaded.as_deref())
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
/// # Returns
///
/// The install report, so the caller can read the version that landed and any warning the checks produced. It used to
/// discard both: the data existed in the pipeline's answer and the command returned nothing.
///
/// # Errors
///
/// `PluginNotInstalled` when no active version exists, `Unsupported` when no registry is configured, and the install
/// pipeline's refusals when the package cannot be fetched or verified.
pub fn plugins_update(state: &AppState, plugin_id: &str) -> Result<InstallReport, IpcError> {
    let report = crate::commands::registry::update(state, plugin_id)?;
    // The stored check belongs to the old version and is no longer returned after the installer changes the active row.
    announce_updated(state, &report);
    Ok(report)
}

/// The updates the last pass found, for a page that was not open when it ran.
///
/// # Errors
///
/// `Internal` when a saved check cannot be read.
pub fn plugins_updates(state: &AppState) -> Result<Vec<UpdateInfo>, IpcError> {
    state.db().plugin_update_checks().map_err(internal)?
        .iter()
        .filter_map(super::plugin_updates::info)
        .collect()
}

/// Publishes the page-level "updated" notice, the same one the file-association path sends.
///
/// # Why the command does this as well as returning the report
///
/// Because the caller that updated from the plugin page does not currently render the report, and a warning about an
/// unsigned package must not depend on which button started the update. The notice is the channel every update already
/// has; the report is there for a caller that wants the detail without listening.
pub(super) fn announce_updated(state: &AppState, report: &InstallReport) {
    state.publish(synra_ipc::event::KernelEvent::Notice {
        message: "notice.updated".to_owned(),
        params: serde_json::json!({
            "pluginId": report.plugin_id.to_string(),
            "version": report.version,
            "warnings": report.warnings,
        }),
    });
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
    report_of(run_install(state, path, false)?)
}

/// The report an install or an update gives the caller.
///
/// One function rather than the same six fields at two call sites: an update's warnings are as much a fact as an
/// install's, and the version that landed is read from the same place in both.
fn report_of(installed: synra_core::install::Installed) -> Result<InstallReport, IpcError> {
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
pub(crate) fn update_directory(
    state: &AppState,
    plugin_id: &str,
    path: &std::path::Path,
) -> Result<InstallReport, IpcError> {
    let id = parse(plugin_id)?;
    migrate_and_unload(state, &id, source_version(path).as_deref())?;
    complete_update(state, &id, run_install(state, path, true))
}

/// Migrates the old instance before replacing its files, then stops it.
pub(super) fn migrate_and_unload(state: &AppState, id: &PluginId, new_version: Option<&str>) -> Result<(), IpcError> {
    // ★ The old instance is asked to migrate **before** its bytes are replaced. Only the old code knows what it wrote,
    // and a migration failure has to leave a working version installed — after the replacement there would be nothing
    // to fall back to.
    let old_version =
        state.plugin_set().ok().and_then(|set| set.lock().version(id).map(str::to_owned));
    if let Some(old) = old_version {
        if let Some(new) = new_version
            && new != old
        {
            let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0);
            let plugins = state.plugin_set()?;
            let mut set = plugins.lock();
            synra_core::plugin::migrate(&mut set, id, &old, new, now).map_err(|reason| {
                IpcError::unknown(synra_protocol::ErrorCode::PluginTrapped)
                    .with_params(serde_json::json!({"pluginId": id, "reason": reason}))
            })?;
        }
        // The old instance is stopped before its files change: a compiled plugin still holding the previous artifact
        // would answer from bytes the update is about to replace.
        apply_unload(state, id);
    }
    Ok(())
}

/// Starts the replacement or restores the old instance when the commit failed, then publishes its card.
pub(super) fn complete_update(
    state: &AppState,
    id: &PluginId,
    outcome: Result<synra_core::install::Installed, IpcError>,
) -> Result<InstallReport, IpcError> {
    let installed = match outcome {
        Ok(installed) => installed,
        Err(error) => {
            // The update did not happen, so the old version is put back. It was unloaded above; reloading is the same
            // call an enable makes, and a failure here is already carried by the original error.
            let _ = apply_lifecycle(state, id);
            announce(state, id);
            return Err(error);
        }
    };
    let id = parse(&installed.plugin_id)?;
    // ★ A refusal here is not a failed update. The new version is on disk and its row is written; what failed is
    // starting it, most often because the new manifest changed and the user has to confirm its capabilities again.
    // Returning an error would tell the user the update did not happen, so the sentence goes on the card instead —
    // the same division the consent command uses.
    let _ = apply_lifecycle(state, &id);
    announce(state, &id);
    report_of(installed)
}

/// The version a package declares, whether it is a directory or a `.synra` archive.
///
/// A directory's manifest is read in place; an archive is unpacked into a temporary directory by
/// `synra_core::install::package_manifest`, which throws the files away and keeps the manifest. Without that, an update
/// from a `.synra` file skipped the migration — the one step that has to run against the old instance before its bytes
/// are replaced.
fn source_version(path: &std::path::Path) -> Option<String> {
    if path.is_file() {
        return synra_core::install::package_manifest(path).ok().map(|manifest| manifest.version);
    }
    let text = std::fs::read_to_string(path.join("manifest.json")).ok()?;
    let manifest = synra_plugin_api::Manifest::from_json(&text).ok()?;
    Some(manifest.version)
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
        // ★ A path that is a file is a `.synra` package; a path that is a directory is the unpacked form. Both are the
        // product's own containers, and the interface's "install a package" flow hands over whichever the user picked.
        match (replace, path.is_file()) {
            (false, false) => synra_core::install::from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (false, true) => synra_core::install::from_package(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (true, false) => synra_core::install::update_from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (true, true) => synra_core::install::update_from_package(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
        }
    };
    let installed = outcome.map_err(|refusal| {
        // ★ The stage the pipeline stopped at, as the label — the refusal already names it, so "installs fail at verify"
        // is a question the recorder can answer without reading a log. A success has one stage, `complete`.
        record_install(state, refusal.stage, "refused");
        IpcError::unknown(refusal.code()).with_params(serde_json::json!({
            "stage": refusal.stage,
            "reason": refusal.reason,
        }))
    })?;
    record_install(state, "complete", "ok");
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
pub fn plugins_uninstall(state: &AppState, plugin_id: &str, remove_data: bool) -> Result<RemoveReport, IpcError> {
    let id = parse(plugin_id)?;
    let removed = state.db().remove_plugin(&id.to_string()).map_err(internal)?;
    if !removed {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id})));
    }
    // ★ The compiled form goes with the row. A plugin whose files are gone but whose instance is still in the set can
    // still match a share and run — from an artifact the user just deleted.
    apply_unload(state, &id);
    // ★ The store measures before and after: `bytes_freed` is what is no longer on disk, not what was there before a
    // locked directory survived. The count beside `leftovers` is the number a user reading "freed" is owed.
    let removal = synra_store::PluginFiles::new(state.paths.clone())
        .remove(&id.to_string(), remove_data)
        .map_err(internal)?;
    Ok(RemoveReport {
        bytes_freed: removal.bytes_freed,
        data_kept: !remove_data,
        // ★ What the operating system would not let go. The row is already gone, so the command cannot fail on a locked
        // file — it reports the path. The page says so beside the freed size; a user who is not told goes looking.
        leftovers: removal.leftovers.iter().map(|path| path.display().to_string()).collect(),
    })
}


/// The plugins this device has, as cards.
///
/// # Errors
///
/// `Internal` when the table or a row cannot be read: an empty list would say "nothing is installed", which is a
/// different and false answer.
pub fn plugins_list(state: &AppState) -> Result<Vec<PluginSnapshot>, IpcError> {
    let rows = state.db().plugins().map_err(internal)?;

    // ★ The two plugin gauges the design names, set where the truth is read rather than incremented on every write: a
    // write that failed half-way leaves no counter to correct, and a database edited by a tool is reflected by the next
    // list. `active` is the user's switch, not "loaded in this process" — a faulted plugin is still enabled until the
    // user says otherwise.
    let installed = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let active = i64::try_from(rows.iter().filter(|row| row.enabled).count()).unwrap_or(i64::MAX);
    state
        .metrics
        .gauge_set(&synra_observe::metrics::SeriesKey::new("plugins_installed", []), installed);
    state
        .metrics
        .gauge_set(&synra_observe::metrics::SeriesKey::new("plugins_active", []), active);

    // ★ Each plugin's sandbox size, as a gauge per plugin. Sampled where the list is read, because the directory grows
    // while the process runs and there is no single write to hook: a plugin's data is whatever it left on disk.
    for row in &rows {
        let bytes = directory_size(&state.paths.plugin_sandbox_dir(&row.plugin_id));
        state.metrics.gauge_set(
            &synra_observe::metrics::SeriesKey::new(
                "plugin_data_bytes",
                [("plugin".to_owned(), row.plugin_id.clone())],
            ),
            i64::try_from(bytes).unwrap_or(i64::MAX),
        );
    }

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
        snapshots.push(snapshot(state, &row, &plugin_id, loaded.as_deref())?);
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
    //
    // ★ And the set follows the row: enabling loads, disabling unloads. Without this the switch would only take effect
    // at the next start, which is a different product than "enable" promises.
    let lifecycle = apply_lifecycle(state, &id);
    announce(state, &id);
    match lifecycle {
        Ok(()) => Ok(()),
        Err(reason) => Err(IpcError::unknown(synra_protocol::ErrorCode::PluginTrapped)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": reason}))),
    }
}

/// Tells the interface that a plugin's card changed, when it can be read back.
///
/// Failure to read the card is not a failure of the command: the write already happened, and refusing the command
/// afterwards would tell the user their change did not take effect when it did — the same reasoning as an audit write
/// that fails after the call it describes.
pub(super) fn announce(state: &AppState, plugin_id: &PluginId) {
    let Ok(rows) = state.db().plugins() else {
        return;
    };
    let Some(row) = rows.into_iter().find(|row| row.plugin_id == plugin_id.to_string()) else {
        return;
    };
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    let Ok(card) = snapshot(state, &row, plugin_id, loaded.as_deref()) else {
        return;
    };
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(card)));
}

/// Builds one card.
///
/// # Errors
///
/// `Internal` for an unreadable update check. A plugin with no readable manifest falls back to its identifier for a name and reports no version,
/// because a manifest that will not parse is something the install path should have refused and the interface needs *a*
/// card either way — a user with a broken plugin needs to see it in order to remove it. The parse failure is not silently
/// swallowed though: it is what puts the card in the `unreadable` state below.
fn snapshot(
    state: &AppState,
    row: &synra_store::PluginRow,
    plugin_id: &PluginId,
    loaded: Option<&[PluginId]>,
) -> Result<PluginSnapshot, IpcError> {
    let manifest = read_manifest(state, &row.plugin_id);
    let is_loaded = loaded.is_some_and(|ids| ids.contains(plugin_id));
    // ★ A plugin can be loaded and still unusable: the health ladder's last rung is `Lifecycle::Faulted`, and the set
    // holds it. Without this read the card said `active` for a plugin every call now refuses — the state word was
    // derived from "is it in the map" rather than from what the map says.
    let fault_reason: Option<String> = state.plugin_set().ok().and_then(|set| {
        let set = set.lock();
        match set.lifecycle(plugin_id).map(synra_plugin_host::Lifecycle::state) {
            Some(synra_plugin_host::State::Faulted { reason, .. }) => Some(reason.clone()),
            _ => None,
        }
    });
    let decisions = state.db().plugin_grants(&row.plugin_id).unwrap_or_default();
    // ★ One read of the load report for both the state word and its sentence. The lock is taken here, the strings are
    // cloned out, and neither helper needs the guard — a second acquisition under the first would be a deadlock.
    let (refused, safe_mode) = {
        let loads = state.plugin_loads();
        (loads.refusal_for(&row.plugin_id).map(str::to_owned), loads.safe_mode.clone())
    };
    let state_word = life_state(row, is_loaded, fault_reason.is_some(), manifest.is_some(), refused.is_some());
    let update_check = state.db().plugin_update_check(&row.plugin_id).map_err(internal)?;
    let update = update_check.as_ref().and_then(super::plugin_updates::info).transpose()?;

    Ok(PluginSnapshot {
        plugin_id: plugin_id.clone(),
        // ★ The same function the removal uses, so the path in the dialog is the directory that will be deleted.
        data_path: state.paths.plugin_sandbox_dir(&row.plugin_id).display().to_string(),
        title: manifest.as_ref().map_or_else(|| row.plugin_id.clone(), |manifest| manifest.title.clone()),
        description: manifest.as_ref().and_then(|manifest| manifest.description.clone()),
        version: manifest.as_ref().map_or_else(String::new, |manifest| manifest.version.clone()),
        publisher: manifest.as_ref().map_or_else(String::new, |manifest| manifest.publisher.id.clone()),
        enabled: row.enabled,
        state: state_word.to_owned(),
        // ★ The sentence behind the state word, from the layer that decided. Without it `unavailable` covers a crash
        // loop, a missing artifact and a build without an engine, and the user is sent to the wrong place.
        state_reason: state_reason(
            state,
            state_word,
            is_loaded,
            refused.as_deref().or(fault_reason.as_deref()),
            safe_mode.as_deref(),
        ),
        hybrid: manifest.as_ref().is_some_and(|manifest| {
            manifest.runtime.headless.is_some() && manifest.runtime.ui.is_some()
        }),
        // ★ Read from the manifest's page entry rather than from `hybrid`: a UI-only plugin has a page and no headless
        // half, and `plugin_ui_open` can open it — the card has to know that before it draws the button.
        has_ui: manifest.as_ref().is_some_and(|manifest| manifest.runtime.ui.is_some()),
        // ★ `None` for a plugin this process has not loaded, and the real report for one it has. A zeroed reading would be
        // indistinguishable from a plugin that genuinely never uses anything — and those two need different reactions from
        // a user deciding whether to keep it. Until this line, the field was `None` for *every* card: the contract had a
        // health shape and the window never filled it, so a plugin degraded by its own refusals looked ordinary.
        health: health_card(state, &row.plugin_id),
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
        // Read the saved answer, never the registry. Only a check of the current installed version is returned.
        update,
        update_checked_at_ms: update_check.as_ref().map(|check| check.checked_at_ms),
    })
}

/// A plugin's quota report, shaped for the card.
///
/// `None` when this process has not loaded the plugin: "never observed" and "observed and fine" are different facts, and a
/// card that showed a zeroed report would be claiming the second while meaning the first.
fn health_card(state: &AppState, plugin_id: &str) -> Option<PluginHealthDto> {
    let report = state.plugin_set().ok()?.lock().report_of(plugin_id)?;
    Some(health_dto(&report))
}

/// The mapping from the engine's report to the contract's shape.
///
/// Split from [`health_card`] so the arithmetic and the field names can be asserted without a loaded plugin: the engine's
/// report is a stable type, and a test can build one.
fn health_dto(report: &synra_plugin_host::quota::QuotaReport) -> PluginHealthDto {
    PluginHealthDto {
        health: report.health.clone(),
        explanation: report.explanation.clone(),
        calls_admitted: report.admitted,
        calls_refused: report.refused,
        // The quota in force, as JSON: it is a shape the interface shows but never reasons about, so mirroring its fields
        // in the contract would be a second definition to keep in step.
        usage: serde_json::to_value(&report.quota).unwrap_or(serde_json::Value::Null),
    }
}

/// Where a plugin is in its life.
///
/// Written as a single function over all four facts rather than derived by the interface, because the interface would
/// then have its own copy of the rule and the two would disagree the first time a case appeared that neither had been
/// written for.
fn life_state(
    row: &synra_store::PluginRow,
    loaded: bool,
    faulted: bool,
    manifest_readable: bool,
    load_refused: bool,
) -> &'static str {
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
    if faulted {
        // ★ The health ladder's last rung. Loaded bytes are not a working plugin: every call is refused above, and the
        // card has to say so rather than showing the green tag a running plugin gets.
        return "faulted";
    }
    if loaded {
        // ★ `active`, the word the design's lifecycle and the page's own colour table already use. It used to say
        // `running`, which matched neither: the page's `active` green tag was unreachable, and a plugin that was loaded
        // and activated showed the same neutral tag as one that had no state at all.
        return "active";
    }
    if load_refused {
        // ★ `faulted`, which the design names and the page already colours red, rather than `unavailable`. The load
        // report has a reason for this plugin: its artifact was missing, its `activate` refused, its consent no longer
        // matches the document. That is a fault, and calling it "unavailable" made the page's `faulted` branch dead code.
        return "faulted";
    }
    // Enabled, trusted, readable, and not loaded — and no refusal to blame the plugin for. `unavailable` rather than
    // `broken`: the shell may simply have no engine, which is a property of this build rather than of the plugin.
    "unavailable"
}

/// Why a card's state word is what it is, when there is something to add.
///
/// # Why the reasons are not derived in the page
///
/// Because they are facts the host holds: which plugin failed to load and what the engine said, whether the whole load
/// pass was skipped after a crash loop, or whether there is no engine at all. A page that guessed from `state` would
/// invent a sentence for each case, and the first case it had not been written for would read as a plugin's fault.
///
/// # Why `unavailable` has a fallback sentence
///
/// A plugin that is enabled, trusted and readable but absent from the set is a fact the report should have explained;
/// when it did not, saying that much is better than leaving the card silent. The reasons above it are the ones that
/// actually happen.
fn state_reason(
    state: &AppState,
    state_word: &str,
    loaded: bool,
    refused: Option<&str>,
    safe_mode: Option<&str>,
) -> Option<String> {
    if loaded {
        return None;
    }
    if state_word == "unreadable" {
        return Some("its manifest could not be read".to_owned());
    }
    if state_word == "faulted" {
        // The load report names what stopped it. The fallback is for the case the report was cleared after the refusal
        // was recorded — the state word is still the honest one, and a bare word beats a stale sentence.
        return Some(refused.map_or_else(|| "it failed to start".to_owned(), str::to_owned));
    }
    if state_word != "unavailable" {
        return None;
    }
    if let Some(reason) = safe_mode {
        return Some(reason.to_owned());
    }
    if let Some(reason) = refused {
        return Some(reason.to_owned());
    }
    if let Some(reason) = &state.engine_refusal {
        return Some(reason.clone());
    }
    if state.plugins.is_none() {
        // The ordinary case in a test, and the honest sentence for a build with no engine: the card has no engine to
        // blame a plugin for, and "unavailable" without this sends the user looking for a broken artifact.
        return Some("this build has no plugin engine".to_owned());
    }
    Some("it is installed and enabled, but this process did not load it".to_owned())
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
/// # ★ Why there is an offset as well
///
/// Because the table holds the whole retention window, and a card that could only ever show the newest twenty rows made
/// "view the full history" a button the design asked for and this build did not have. The offset is applied in SQL, so
/// "show more" walks the index rather than re-reading the rows the user has already seen.
///
/// # ★ Why this reads the table rather than the ring
///
/// The ring is this process's last ten thousand calls; the table is the history. Both hosts install an audit sink
/// ([`crate::plugin_host::audit_sink`]) that writes every drained record, so the table is a **superset** of the ring —
/// it carries the calls from before the last restart. Reading the ring would answer "nothing" on a fresh start even though
/// the user can still see yesterday's refusals in a diagnostic bundle, and the design's whole reason for the durable half
/// is that "recent activity" survives a restart.
///
/// It also stops needing a plugin engine: the trail outlives the engine, so a build without one can still answer what
/// this database recorded.
///
/// # Errors
///
/// [`IpcError`] when the table cannot be read.
#[allow(clippy::result_large_err)]
pub fn plugins_audit(
    state: &AppState,
    plugin_id: &PluginId,
    limit: u32,
    offset: u32,
) -> Result<Vec<AuditEntryDto>, IpcError> {
    let records = state
        .db()
        .audit_trail_for(plugin_id.as_str(), offset, limit)
        .map_err(internal)?;
    Ok(records.iter().map(record_card).collect())
}

/// Forgets one plugin's audit history.
///
/// # Why the durable rows and the ring
///
/// They are two copies of one trail. Clearing only the table would let the diagnostics bundle — which reads the ring —
/// resurrect what the user just deleted; clearing only the ring would bring the rows back on the next read. A build
/// without an engine has no ring, so that half is skipped rather than refused.
///
/// # Errors
///
/// [`IpcError`] when the table refuses the delete. The ring cannot fail.
#[allow(clippy::result_large_err)]
pub fn plugins_audit_clear(state: &AppState, plugin_id: &PluginId) -> Result<(), IpcError> {
    if let Ok(set) = state.plugin_set() {
        set.lock().clear_audit(plugin_id);
    }
    state.db().forget_audit_for(plugin_id.as_str()).map_err(internal)?;
    Ok(())
}

/// The configuration key that sets how long recorded calls are kept, in days.
///
/// The design names the setting `audit_retention_days`; the configuration addresses keys by dotted path, and every other
/// key this shell reads is camelCase after the first segment (`transfers.confirmCancel`), so this one is spelled the same
/// way.
pub(crate) const AUDIT_RETENTION_KEY: &str = "audit.retentionDays";

/// How long a recorded call is kept when nothing says otherwise.
const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 7;

/// The shortest and longest retention a caller can ask for.
///
/// A day is the smallest unit the setting has; ten years is past the point where the table is a diagnostic aid and into
/// "why is this database a gigabyte". The clamp is here rather than in the screen because a configuration file can be
/// edited by hand, and a negative or absurd number must not become an arithmetic overflow in the prune.
const MIN_AUDIT_RETENTION_DAYS: u32 = 1;
const MAX_AUDIT_RETENTION_DAYS: u32 = 3_650;

/// The configured retention, clamped, or the design's default when the key is absent or unreadable.
///
/// # Why an unreadable value is the default rather than a refusal
///
/// Because this is read on the start-up path, and a bad setting in a file must not stop the window from opening. The
/// screen that writes it round-trips `config_get`, so the user sees the stored value; a value this function cannot read is
/// one the screen cannot have produced.
fn audit_retention_days(state: &AppState) -> u32 {
    let stored = {
        let db = state.db();
        db.config().ok().and_then(|rows| {
            rows.into_iter()
                .find(|row| row.key == AUDIT_RETENTION_KEY)
                .and_then(|row| serde_json::from_str::<u64>(&row.value).ok())
        })
    };
    match stored {
        Some(days) => u32::try_from(days).unwrap_or(u32::MAX).clamp(MIN_AUDIT_RETENTION_DAYS, MAX_AUDIT_RETENTION_DAYS),
        None => DEFAULT_AUDIT_RETENTION_DAYS,
    }
}

/// Forgets recorded calls older than the retention window, and answers how many went.
///
/// Called once per [`AppState`](crate::state::AppState), like the registry document prune: a table that only grows costs
/// more than the history is worth, and no request should have to remember to sweep it.
pub(crate) fn prune_audit(state: &AppState, now_ms: i64) -> usize {
    let days = audit_retention_days(state);
    let window_ms = i64::from(days) * 24 * 60 * 60 * 1_000;
    match state.db().forget_audit_before(now_ms.saturating_sub(window_ms)) {
        Ok(removed) => removed,
        Err(error) => {
            // A failure to prune is a diagnostic, not a reason to refuse to start.
            eprintln!("synra: recorded calls could not be pruned: {error}");
            0
        }
    }
}

/// One recorded call, as the interface shows it.
///
/// ★ The row stores the outcome's **name**, so this has to read it back. [`AuditOutcome::from_stable_name`] is that
/// parser, and it lives beside `as_str` so the table's spelling has one definition rather than two.
///
/// A name this build does not know is reported rather than guessed at: `allowed: false` with the name as the reason.
/// Presenting an outcome from a newer build as "allowed" is the one answer that could hide a refusal.
///
/// ★ The DTO's `trace_id` now comes from the row. It used to be `None` because the row had no such column and the session
/// that made the call was never given the action's trace — two halves of one gap, closed together: `call_traced` stamps the
/// drained entries, the migration adds the column, and this is the mapping that finally has something to read.
fn record_card(record: &synra_store::records::AuditRecord) -> AuditEntryDto {
    let outcome = synra_observe::audit::AuditOutcome::from_stable_name(&record.outcome);
    AuditEntryDto {
        at_ms: record.at_ms,
        capability: record.capability.clone(),
        allowed: outcome.is_some_and(synra_observe::audit::AuditOutcome::was_allowed),
        // The outcome's stable name, for a refusal only. An allowed call has no reason to give.
        reason: match outcome {
            Some(outcome) if outcome.is_refusal() => Some(outcome.as_str().to_owned()),
            Some(_) => None,
            None => Some(record.outcome.clone()),
        },
        elapsed_ms: record.elapsed_ms.and_then(|elapsed| u64::try_from(elapsed).ok()),
        trace_id: record.trace_id.clone(),
    }
}

/// Records one finished install pipeline, labelled with where it ended.
///
/// `stage` is the refusal's own stable name (`source`, `manifest`, `verify`, …) or `complete`; `result` is `ok` or
/// `refused`. One sample per attempt, not per step: what the counter answers is "where do installs stop", and a step
/// that succeeded before a later one refused would otherwise make every attempt look like a pass.
fn record_install(state: &AppState, stage: &str, result: &str) {
    state.metrics.counter_inc(&synra_observe::metrics::SeriesKey::new(
        "plugin_install_total",
        [
            ("result".to_owned(), result.to_owned()),
            ("stage".to_owned(), stage.to_owned()),
        ],
    ));
}

/// The bytes under a directory, or zero when it cannot be read.
///
/// ★ Errors become zero rather than failing the list: a size is a diagnostic, and a plugin whose sandbox is unreadable is
/// still a plugin the list can describe. A directory that does not exist is zero too — "no data yet" and "no permission"
/// are the same number here, and the gauge is not the place to tell them apart.
fn directory_size(root: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_size(&entry.path()),
            Ok(_) => entry.metadata().map_or(0, |metadata| metadata.len()),
            Err(_) => 0,
        })
        .fold(0u64, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIT_RETENTION_KEY, announce_updated, discover_plugins, install_directory, load_installed_plugins, plugins_audit,
        plugins_audit_clear, plugins_catalog_search, plugins_check_update, plugins_consent_respond, plugins_install,
        plugins_list, plugins_preview, plugins_publisher_decide, plugins_publishers, prune_audit, record_card,
    };
    use synra_ipc::{CatalogQuery, InstallSourceDto};
    use synra_protocol::PluginId;
    use crate::testing;

    const ID: &str = "@synra-plugin/chat";

    #[test]
    fn a_faulted_plugin_is_not_drawn_as_active() {
        // ★ The set holds the bytes and the lifecycle holds the fault. "Loaded" was the whole question before, so a
        // plugin the health ladder had stopped calling still got the green tag of one that was running.
        let row = synra_store::PluginRow {
            plugin_id: ID.to_owned(),
            enabled: true,
            trusted: true,
            health: "faulted".to_owned(),
            installed_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(super::life_state(&row, true, true, true, false), "faulted");
        assert_eq!(super::life_state(&row, true, false, true, false), "active");
    }

    /// A state with a scripted engine, because installing needs one and the fixture builds a shell without.
    ///
    /// The backend answers from a table rather than compiling Wasm: what is being tested is the shell's command — the
    /// source it accepts, the report it returns and the row that lands — and the pipeline behind it has its own tests.
    fn with_engine(state: crate::state::AppState) -> crate::state::AppState {
        with_scripted_engine(state).0
    }

    /// The same engine, with the backend kept so a test can script one entry point.
    fn with_scripted_engine(
        state: crate::state::AppState,
    ) -> (crate::state::AppState, synra_plugin_host::ScriptedBackend) {
        let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::with_platform(
            std::sync::Arc::clone(&state.db),
            std::sync::Arc::clone(&state.platform),
        ));
        let scripted = synra_plugin_host::ScriptedBackend::new();
        let backend = synra_plugin_host::Backend::new(std::sync::Arc::new(scripted.clone()));
        (
            state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(
                synra_core::dispatch::PluginSet::new(backend, host),
            ))),
            scripted,
        )
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
        let archive = tgz(&[
            ("package/manifest.json", manifest.as_slice()),
            ("package/plugin.wasm", b"\0asm\x01\0\0\0"),
        ]);
        // ★ The document declares the digest of the bytes it points at, and the install checks them: a fake registry with
        // no integrity is now refused before anything is unpacked, which is the property the test below would hide if
        // this one omitted it.
        let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();
        let document = serde_json::json!({
            "name": "@acme/hello",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@acme/hello",
                    "version": "1.0.0",
                    "dist": { "tarball": format!("{base}/pkg.tgz"), "integrity": integrity },
                    "synra": manifest_json
                }
            }
        })
        .to_string();
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
        // ★ And the pipeline's one counter: the report says it worked, the recorder says the same. A success is the
        // `complete` stage — the refusal stages only appear when something stopped.
        assert_eq!(
            state.metrics.snapshot().get("plugin_install_total{result=ok,stage=complete}"),
            Some(&synra_observe::metrics::Value::Counter(1))
        );
    }

    #[test]
    fn installing_a_package_file_uses_the_same_pipeline() {
        // ★ A `.synra` is the shape sync and the registry both produce, and a user can have one on disk. Passing it to the
        // directory-only path used to refuse with "not a directory" even though the unpacker and the installer were both
        // already there.
        let (root, state) = testing::file_state();
        let state = with_engine(state);
        let source = package(root.path());
        let archive = root.path().join("hello.synra");
        synra_core::install::package_directory(&source, &archive).expect("the package is written");

        let report = plugins_install(
            &state,
            &InstallSourceDto::Path { path: archive.to_string_lossy().into_owned() },
        )
        .expect("the package installs");

        assert_eq!(report.plugin_id.as_str(), "@acme/hello");
        assert_eq!(report.version, "1.0.0");
        assert!(!report.enabled);
        let installed = state.paths.plugin_dir("@acme/hello");
        assert!(
            installed.join("source.json").is_file(),
            "no source note in {installed:?}: {:?}",
            std::fs::read_dir(&installed).map(|entries| entries
                .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                .collect::<Vec<_>>())
        );
        let note: serde_json::Value =
            serde_json::from_slice(&std::fs::read(installed.join("source.json")).expect("the note"))
                .expect("JSON");
        assert_eq!(
            std::fs::canonicalize(note["path"].as_str().expect("a path")).expect("canonical"),
            std::fs::canonicalize(&archive).expect("a canonical archive"),
            "the installed note names the package, not the unpacked temporary directory"
        );
    }

    #[test]
    fn a_refused_install_is_counted_with_the_stage_that_refused_it() {
        // ★ The interesting half: a counter that only counts successes says "installs are fine" while every attempt is
        // refused at `source`. The stage is the refusal's own name, the same one its `IpcError` params carry.
        let (root, state) = testing::file_state();
        let state = with_engine(state);

        let error = install_directory(&state, &root.path().join("not-a-package")).expect_err("refused");

        assert_eq!(error.params["stage"], "source");
        assert_eq!(
            state.metrics.snapshot().get("plugin_install_total{result=refused,stage=source}"),
            Some(&synra_observe::metrics::Value::Counter(1))
        );
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
        let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();

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
                    "dist": { "tarball": format!("{first}/pkg.tgz"), "integrity": integrity }
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
        let integrity = synra_core::ObjectDigest::of(&archive).as_sri().to_string();

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
                // Relative to the index's own URL: the client resolves it, so the author need not know the bucket.
                "tarball": "pkg.tgz",
                // And the digest of the bytes it names; without one the install refuses before unpacking.
                "integrity": integrity,
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
    fn an_npm_packument_is_cached_by_its_etag() {
        // ★ The validators are the registry's whether it speaks npm or serves a bucket. A check-update that re-reads the
        // whole packument for a document that has not changed is a round trip this side chose not to save.
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
        let body = serde_json::json!({
            "name": "@acme/hello",
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "2.0.0": {
                    "name": "@acme/hello",
                    "version": "2.0.0",
                    "dist": { "tarball": format!("{base}/pkg.tgz") },
                    "synra": {
                        "id": "@acme/hello",
                        "version": "2.0.0",
                        "title": "Hello",
                        "publisher": { "id": "@acme" },
                        "sdk": { "min": "0.1.0" },
                        "wit": synra_plugin_host::host_world(),
                        "capabilities": [{ "cap": "log:*" }],
                        "runtime": { "headless": { "artifact": "plugin.wasm", "stateful": false } }
                    }
                }
            }
        })
        .to_string();
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&requests);
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
                if headers.to_ascii_lowercase().contains("if-none-match: \"p1\"") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    );
                }
                else {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\netag: \"p1\"\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body.as_bytes());
                }
            }
        });
        state
            .db()
            .set_config("plugins.registry", &serde_json::json!(base), 1_000)
            .expect("the registry is configured");

        let first = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");
        let second = plugins_check_update(&state, "@acme/hello").expect("a check").expect("an update");

        assert_eq!(first.version, "2.0.0");
        assert_eq!(second.version, "2.0.0", "the 304 answered from the cache");
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

    /// A recorded row, filled in with the fields the mapping reads.
    fn row(plugin: &str, capability: &str, outcome: &str, at_ms: i64) -> synra_store::records::AuditRecord {
        synra_store::records::AuditRecord {
            plugin_id: plugin.to_owned(),
            capability: capability.to_owned(),
            outcome: outcome.to_owned(),
            at_ms,
            elapsed_ms: Some(7),
            trace_id: None,
        }
    }

    #[test]
    fn a_recorded_call_is_mapped_from_its_row() {
        // ★ What this command is responsible for. The table's own read is tested where it lives; what is left here is the
        // mapping, and a wrong mapping looks right — an allowed call reported with a reason, or a refusal reported as
        // allowed.
        let allowed = record_card(&row(ID, "log:info", "allowed", 1_700_000_000_000));
        assert_eq!(allowed.capability, "log:info");
        assert!(allowed.allowed);
        assert_eq!(allowed.elapsed_ms, Some(7));
        assert_eq!(allowed.reason, None, "an allowed call has no reason to give");

        let denied = record_card(&row(ID, "device:list", "denied", 1_700_000_000_001));
        assert!(!denied.allowed);
        // The outcome's stable name, which the interface translates — not a sentence written here.
        assert_eq!(denied.reason.as_deref(), Some("denied"));
        // A row recorded without an action behind it stays without one — `None` here is a fact about the call, not a
        // mapping that failed.
        assert_eq!(denied.trace_id, None);

        // ★ And a row with one gives the DTO one. The column is new; this is the half that proves the mapping reads it
        // rather than still answering `None` from an older shape.
        let mut traced = row(ID, "device:list", "allowed", 1_700_000_000_003);
        traced.trace_id = Some("01a150c8-c332-768d-9dd1-981502aa1854".to_owned());
        assert_eq!(
            record_card(&traced).trace_id.as_deref(),
            Some("01a150c8-c332-768d-9dd1-981502aa1854")
        );

        // ★ "Permitted by the gate, failed in the operation" is not a refusal — `AuditOutcome::was_allowed` says so, and a
        // mapping that compared the name against `"allowed"` would draw this call as blocked.
        let failed = record_card(&row(ID, "network:fetch", "failed", 1_700_000_000_002));
        assert!(failed.allowed, "the gate permitted this call");
        assert_eq!(failed.reason, None);

        // A name from a newer build is reported rather than guessed at: presenting it as allowed could hide a refusal.
        let unknown = record_card(&row(ID, "device:list", "from-a-newer-build", 1_700_000_000_003));
        assert!(!unknown.allowed);
        assert_eq!(unknown.reason.as_deref(), Some("from-a-newer-build"));
    }

    #[test]
    fn the_audit_reads_the_trail_that_outlives_the_engine() {
        // ★ The fixture has no engine, and that must not matter: the table is written by whichever host ran the call, so a
        // build that cannot run plugins can still answer what this database recorded. The old `Unsupported` answer was
        // true only while the in-memory ring was the whole trail.
        let state = crate::testing::state();
        state.db().record_audit(&row(ID, "device:list", "allowed", 5_000)).expect("recorded");

        let rows = plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].capability, "device:list");
        assert_eq!(rows[0].at_ms, 5_000);
    }

    #[test]
    fn the_audit_read_can_skip_the_rows_a_page_already_showed() {
        // ★ "Show more" is wired through this parameter. A command that ignored it would repeat the first page, which is
        // the one failure a list that appends rather than replaces would show as duplicated rows rather than as nothing.
        let state = crate::testing::state();
        for at in [1_000, 2_000, 3_000] {
            state.db().record_audit(&row(ID, "device:list", "allowed", at)).expect("recorded");
        }

        let rows = plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 1, 1).expect("read");

        assert_eq!(rows.len(), 1, "one row asked for, one row answered");
        assert_eq!(rows[0].at_ms, 2_000, "and it is the second-newest, not the newest again");
    }

    #[test]
    fn clearing_forgets_the_durable_rows_and_leaves_another_plugin() {
        // The same rule as the ring, now on the copy that survives a restart.
        let state = crate::testing::state();
        state.db().record_audit(&row(ID, "device:list", "allowed", 5_000)).expect("recorded");
        state.db().record_audit(&row("@acme/other", "log:info", "allowed", 6_000)).expect("recorded");

        plugins_audit_clear(&state, &PluginId::parse(ID).expect("a plugin id")).expect("cleared");

        assert_eq!(state.db().audit_count().expect("counted"), 1);
        assert!(
            plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0)
                .expect("read")
                .is_empty()
        );
    }

    #[test]
    fn old_calls_are_pruned_and_recent_ones_kept() {
        // ★ The table is written on every gated call and nothing in this shell swept it, so a long-running install would
        // grow it for ever. With no configured window the design's seven days is the rule.
        let state = crate::testing::state();
        let day = 24 * 60 * 60 * 1_000;
        let now = 30 * day;
        state.db().record_audit(&row(ID, "device:list", "allowed", now - 10 * day)).expect("recorded");
        state.db().record_audit(&row(ID, "device:list", "allowed", now - 2 * day)).expect("recorded");

        assert_eq!(prune_audit(&state, now), 1);

        let rows = plugins_audit(&state, &PluginId::parse(ID).expect("a plugin id"), 10, 0).expect("read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].at_ms, now - 2 * day, "the recent call stayed");
    }

    #[test]
    fn the_retention_window_comes_from_the_configuration() {
        // ★ The setting existed only as a constant. Reading it is what turns "the design names a key" into the key doing
        // something: a one-day window has to remove the two-day-old row that the default seven kept in the test above.
        use synra_store::ConfigChange;

        let state = crate::testing::state();
        let day = 24 * 60 * 60 * 1_000;
        let now = 30 * day;
        state.db().record_audit(&row(ID, "device:list", "allowed", now - 10 * day)).expect("recorded");
        state.db().record_audit(&row(ID, "device:list", "allowed", now - 2 * day)).expect("recorded");
        state
            .db()
            .apply_config(
                &[ConfigChange {
                    key: AUDIT_RETENTION_KEY.to_owned(),
                    value: Some(serde_json::json!(1)),
                }],
                now,
            )
            .expect("the setting is written");

        assert_eq!(prune_audit(&state, now), 2, "a one-day window removes both calls");
    }

    #[test]
    fn a_retention_outside_the_sane_range_is_clamped_rather_than_trusted() {
        // ★ The file can be edited by hand, and `0` would delete today's calls — including the refusal the user just
        // looked at. The clamp is what makes the setting a window rather than a way to empty the table from a typo.
        use synra_store::ConfigChange;

        let state = crate::testing::state();
        let day = 24 * 60 * 60 * 1_000;
        let now = 30 * day;
        state.db().record_audit(&row(ID, "device:list", "allowed", now)).expect("recorded");
        state
            .db()
            .apply_config(
                &[ConfigChange {
                    key: AUDIT_RETENTION_KEY.to_owned(),
                    value: Some(serde_json::json!(0)),
                }],
                now,
            )
            .expect("the setting is written");

        assert_eq!(prune_audit(&state, now), 0, "zero becomes the one-day minimum, and today's call stays");
    }

    #[test]
    fn listing_plugins_sets_the_installed_and_active_gauges() {
        // ★ Set where the truth is read rather than incremented on every write: a write that failed half-way leaves no
        // counter to correct. The fixture installs one enabled plugin; a second, disabled row makes the two numbers
        // differ — an assertion that only checked one gauge would pass even if both were fed the same value.
        use synra_observe::metrics::Value;

        let (_root, state) = installed();
        state
            .db()
            .insert_plugin("@acme/other", false, false, "installed", testing::FIXED_NOW_MS, testing::FIXED_NOW_MS)
            .expect("a second row");

        let _ = plugins_list(&state).expect("the list");

        let snapshot = state.metrics.snapshot();
        assert_eq!(snapshot.get("plugins_installed"), Some(&Value::Gauge(2)));
        assert_eq!(snapshot.get("plugins_active"), Some(&Value::Gauge(1)), "only the enabled one is active");
    }

    #[test]
    fn listing_plugins_reports_the_bytes_each_sandbox_holds() {
        // ★ The storage group's per-plugin gauge: the design names it, and nothing measured it. Nested files are in the
        // fixture because a top-level-only walk would pass a test that only wrote one file at the root.
        use synra_observe::metrics::Value;

        let (_root, state) = installed();
        let sandbox = state.paths.plugin_sandbox_dir(ID);
        std::fs::create_dir_all(sandbox.join("nested")).expect("the sandbox");
        std::fs::write(sandbox.join("cache.bin"), [1u8; 10]).expect("a file");
        std::fs::write(sandbox.join("nested").join("more.bin"), [2u8; 5]).expect("a nested file");

        let _ = plugins_list(&state).expect("the list");

        assert_eq!(
            state.metrics.snapshot().get("plugin_data_bytes{plugin=@synra-plugin/chat}"),
            Some(&Value::Gauge(15))
        );
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
        std::fs::write(data.join("cache.bin"), [7u8; 5]).expect("some bytes");

        let report = super::plugins_uninstall(&state, ID, false).expect("uninstalls");

        assert!(
            state.db().plugins().expect("rows").iter().all(|row| row.plugin_id != ID),
            "the row is gone"
        );
        assert!(!state.paths.plugin_dir(ID).exists(), "and the artifacts with it");
        assert!(data.exists(), "but the data stays unless asked for");
        // ★ The report the design names and nothing returned: the artifact's bytes were freed, and the five bytes in
        // the sandbox were **not** — `data_kept` is the same fact in a sentence the page can print.
        assert!(report.data_kept, "the data stayed");
        assert_eq!(report.bytes_freed as usize, MANIFEST.len(), "only the artifact was freed");
        assert!(report.leftovers.is_empty(), "a wholesale removal leaves nothing behind");
    }

    #[test]
    fn asking_for_the_data_removes_it_as_well() {
        // ★ The other half of the `remove_data` parameter, which the page never sends — and which nothing tested. It is the
        // one value of it that cannot be undone, so an untested `true` is the worst one to leave untested.
        let (_root, state) = installed();
        let data = state.paths.plugin_sandbox_dir(ID);
        std::fs::create_dir_all(&data).expect("some plugin data");
        std::fs::write(data.join("cache.bin"), [7u8; 5]).expect("some bytes");

        let report = super::plugins_uninstall(&state, ID, true).expect("uninstalls");

        assert!(!data.exists(), "the data went with the plugin");
        assert!(!report.data_kept, "and the report says so");
        assert_eq!(
            report.bytes_freed as usize,
            MANIFEST.len() + 5,
            "artifact and sandbox both counted"
        );
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
        // ★ The path the removal dialog shows is the directory the removal would delete: both come from
        // `plugin_sandbox_dir`, and this assertion is what keeps the card from inventing a second spelling of it.
        assert_eq!(
            card.data_path,
            state.paths.plugin_sandbox_dir(ID).display().to_string(),
            "the card names the sandbox the removal deletes"
        );
        // A shell with no engine has observed nothing, which is not the same as "healthy".
        assert!(card.health.is_none(), "no engine, no report");
    }

    #[test]
    fn a_loaded_plugins_card_carries_its_health() {
        // ★ The field the contract has always had and the shell never filled: every card said `None`, so a plugin the
        // quota had degraded looked exactly like one that had never run. The report comes from the same budget the gate
        // counts into, so this also proves the card and the gate are looking at one object.
        let (_root, state) = installed();
        let state = with_engine(state);
        {
            let set = state.plugin_set().expect("an engine");
            let mut set = set.lock();
            set.load(synra_core::dispatch::PluginSpec {
                id: PluginId::parse(ID).expect("an id"),
                version: "1.0.0".to_owned(),
                wasm: b"\0asm\x01\0\0\0".to_vec(),
                grants: synra_capability::CapabilitySet::new(),
                policy: synra_plugin_host::InstancePolicy::Stateless,
                limit: 1,
                quota: synra_plugin_host::quota::Quota::default(),
            })
            .expect("loads");
        }

        let cards = plugins_list(&state).expect("the list");
        let card = cards.iter().find(|card| card.plugin_id.as_str() == ID).expect("the card");
        let health = card.health.as_ref().expect("a loaded plugin has a report");
        assert_eq!(health.health, "good");
        assert_eq!(health.calls_admitted, 0);
        assert!(!health.usage.is_null(), "the quota in force is carried");
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

    /// Puts a loadable plugin on disk and records the decision that lets it run.
    ///
    /// ★ The directory comes from `Paths::plugin_dir`, the same function the installer, removal and sweep use. A fixture
    /// that joined the raw id here would put the manifest somewhere the loader never looks, and the test would then be
    /// asserting a layout no installation has.
    fn write_loadable_files(state: &crate::state::AppState) -> String {
        let manifest = format!(
            r#"{{"id": "{ID}", "version": "1.0.0", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
            synra_plugin_host::host_world()
        );
        let directory = state.paths.plugin_dir(ID);
        std::fs::create_dir_all(&directory).expect("the plugin directory");
        std::fs::write(directory.join("manifest.json"), &manifest).expect("a manifest");
        std::fs::write(directory.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("an artifact");
        manifest
    }

    fn loadable(state: &crate::state::AppState) {
        let manifest = write_loadable_files(state);
        state
            .db()
            .insert_plugin(ID, true, true, "installed", testing::FIXED_NOW_MS, testing::FIXED_NOW_MS)
            .expect("a plugin row");
        state
            .db()
            .record_grants(
                ID,
                &[("log:*".to_owned(), true)],
                testing::FIXED_NOW_MS,
                &synra_plugin_api::manifest_digest(&manifest),
            )
            .expect("the decisions");
    }

    /// A directory package for a newer version, as the registry updater unpacks one.
    fn write_update_source(root: &std::path::Path, version: &str) -> std::path::PathBuf {
        let directory = root.join(format!("update-{version}"));
        std::fs::create_dir_all(&directory).expect("a package directory");
        let manifest = format!(
            r#"{{"id": "{ID}", "version": "{version}", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
            synra_plugin_host::host_world()
        );
        std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
        std::fs::write(directory.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("an artifact");
        directory
    }

    #[test]
    fn the_shell_loads_an_installed_plugin_rather_than_leaving_its_set_empty() {
        // ★ The whole point of this round: before it, `with_plugins_or_refuse` built an empty `PluginSet` and never
        // loaded anything, so an installed plugin was listed, could be enabled, and was never askable. The assertion is
        // on the set itself, because that is what an intent matches against.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        loadable(&state);

        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        assert_eq!(summary.len(), 1, "refused: {:?}", summary.refused);
        let id = PluginId::parse(ID).expect("an id");
        assert!(state.plugin_set().expect("an engine").lock().contains(&id));

        let cards = plugins_list(&state).expect("the list");
        let card = cards.iter().find(|card| card.plugin_id.as_str() == ID).expect("the card");
        assert_eq!(card.state, "active");
        assert_eq!(card.state_reason, None, "a running plugin needs no excuse");
    }

    #[test]
    fn safe_mode_is_visible_on_the_card_rather_than_just_an_empty_set() {
        // ★ The M4 acceptance: a plugin that crashes the process three times must not be started a fourth, and the user
        // has to be told why the plugin they can see is not running. Reaching the verdict through three reports is the
        // point — hand-building `SafeMode` would only prove that a made-up value returns false.
        let (root, state) = testing::file_state();
        let state = with_engine(state);
        loadable(&state);
        let crashes = synra_observe::crash::CrashStore::at(root.path().join("crashes"));
        for offset in 0..synra_observe::crash::SAFE_MODE_THRESHOLD {
            crashes
                .write(&synra_observe::crash::CrashReport::new(
                    testing::FIXED_NOW_MS + i64::try_from(offset).expect("a small offset"),
                    "0.1.0",
                    Some("src/plugin.rs:42".to_owned()),
                    Some("the plugin panicked".to_owned()),
                    None,
                    None,
                    None,
                ))
                .expect("the report is written");
        }
        let verdict = crashes.verdict(testing::FIXED_NOW_MS);
        assert!(!verdict.allows_plugins(), "three crashes did not reach safe mode: {verdict:?}");

        let summary = load_installed_plugins(
            &state,
            &verdict,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        assert!(summary.is_empty(), "safe mode loaded something: {:?}", summary.loaded);
        let state = state.with_plugin_loads(summary);

        let cards = plugins_list(&state).expect("the list");
        let card = cards.iter().find(|card| card.plugin_id.as_str() == ID).expect("the card");
        assert_eq!(card.state, "unavailable");
        let reason = card.state_reason.as_deref().expect("safe mode must say why");
        assert!(reason.contains("crashed 3 times"), "{reason}");
    }

    #[test]
    fn a_plugin_that_failed_to_load_says_why_on_its_card() {
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        let directory = state.paths.plugin_dir(ID);
        loadable(&state);
        std::fs::remove_file(directory.join("plugin.wasm")).expect("the artifact is removed");

        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        assert_eq!(summary.refused.len(), 1, "{summary:?}");
        let state = state.with_plugin_loads(summary);

        let cards = plugins_list(&state).expect("the list");
        let card = cards.iter().find(|card| card.plugin_id.as_str() == ID).expect("the card");
        // ★ A recorded refusal is `faulted`, which the design names and the page already colours red. It used to say
        // `unavailable`, leaving the page's faulted branch unreachable and making a broken plugin look like a build
        // without an engine.
        assert_eq!(card.state, "faulted");
        let reason = card.state_reason.as_deref().expect("a refusal must say why");
        assert!(reason.contains("artifact"), "{reason}");
    }

    #[test]
    fn a_directory_copied_into_this_shells_data_dir_becomes_a_card() {
        // ★ The shell has its own `plugins/` directory, and before this it never scanned it: a directory an author
        // copied in was invisible in the window while the same directory beside the daemon appeared. Discovery records
        // and does not trust, so the card is `awaiting_consent` rather than running.
        let (_root, state) = testing::file_state();
        write_loadable_files(&state);
        assert!(state.db().plugins().expect("rows").is_empty(), "nothing is recorded yet");

        let found = discover_plugins(&state, testing::FIXED_NOW_MS);

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].plugin_id, ID);
        assert!(found[0].problem.is_none(), "{found:?}");
        let cards = plugins_list(&state).expect("the list");
        let card = cards.first().expect("the discovered plugin");
        // A discovered directory is written disabled and untrusted, and `disabled` is the first word: the user has not
        // switched it on, so the fact that consent has also not been answered is not the card's headline.
        assert_eq!(card.state, "disabled");
        assert!(!card.enabled && card.granted.is_empty(), "a copied directory is not an approval");
    }

    #[test]
    fn an_update_asks_the_old_version_to_migrate_before_replacing_it() {
        // ★ The data being moved is the old plugin's, and the old code is the only code that knows what it wrote. A
        // migration that runs after the replacement has no working version to fall back to.
        let (root, state) = testing::file_state();
        let state = with_engine(state);
        loadable(&state);
        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        let state = state.with_plugin_loads(summary);
        let id = PluginId::parse(ID).expect("an id");
        assert!(state.plugin_set().expect("an engine").lock().contains(&id));

        let source = write_update_source(root.path(), "2.0.0");
        super::update_directory(&state, ID, &source).expect("the update is applied");

        let active = state.db().active_plugin(ID).expect("the version table").expect("an active version");
        assert_eq!(active.version, "2.0.0");
    }

    #[test]
    fn a_migration_that_refuses_stops_the_update_and_leaves_the_old_version() {
        let (root, state) = testing::file_state();
        let (state, backend) = with_scripted_engine(state);
        loadable(&state);
        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        let state = state.with_plugin_loads(summary);
        let id = PluginId::parse(ID).expect("an id");
        backend.script_for(
            ID,
            &synra_plugin_host::Work::Migrate {
                from_version: "1.0.0".to_owned(),
                to_version: "2.0.0".to_owned(),
            },
            synra_plugin_host::Scripted::Failed {
                code: "invalidArgument".to_owned(),
                message: "this data cannot move".to_owned(),
            },
        );

        let source = write_update_source(root.path(), "2.0.0");
        let refused = super::update_directory(&state, ID, &source).expect_err("the migration refused");
        assert_eq!(refused.code, synra_protocol::ErrorCode::PluginTrapped);
        let reason = refused.params["reason"].as_str().expect("the refusal names the reason");
        assert!(
            reason.contains("this data cannot move"),
            "the plugin's own sentence is carried: {reason}"
        );
        // The old version is still installed and still loaded: a failed migration changed nothing the user can see.
        // The manifest on disk is the evidence that `run_install` was never reached, and the set's version is the
        // evidence that the old compiled form is still the one answering.
        let manifest = std::fs::read_to_string(state.paths.plugin_dir(ID).join("manifest.json"))
            .expect("the old manifest");
        assert!(manifest.contains("\"1.0.0\""), "the update wrote a new version despite the refusal: {manifest}");
        assert_eq!(state.plugin_set().expect("an engine").lock().version(&id), Some("1.0.0"));
    }

    #[test]
    fn retrying_a_faulted_plugin_loads_it_once_the_cause_is_gone() {
        // ★ The retry path: the row is already enabled and trusted, so the only thing that has to change is the set.
        // This is what the page's Retry button calls, and it must not need a flag to flip.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        let directory = state.paths.plugin_dir(ID);
        loadable(&state);
        let artifact = directory.join("plugin.wasm");
        let bytes = std::fs::read(&artifact).expect("the artifact");
        std::fs::remove_file(&artifact).expect("the artifact is removed");
        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        let state = state.with_plugin_loads(summary);
        let cards = plugins_list(&state).expect("the list");
        assert_eq!(cards[0].state, "faulted");

        std::fs::write(&artifact, bytes).expect("the artifact is back");
        let card = super::plugins_reload(&state, ID).expect("the reload answers with the card");

        assert_eq!(card.state, "active", "the retry did not load the plugin: {card:?}");
        assert!(
            state.plugin_set().expect("an engine").lock().contains(&PluginId::parse(ID).expect("an id")),
            "the card says active but the set is empty"
        );
    }

    #[test]
    fn retrying_a_plugin_that_is_not_installed_is_a_refusal() {
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        let refused = super::plugins_reload(&state, ID).expect_err("no row to reload");
        assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
    }

    #[test]
    fn a_shell_without_an_engine_says_so_on_the_card() {
        // ★ "This build cannot run plugins" and "this plugin is broken" are different answers, and only one of them
        // sends the user to the right place. A shell built without an engine is a real configuration, not a failure.
        let (_root, state) = installed();
        state.db().set_plugin_trusted(ID, true, testing::FIXED_NOW_MS).expect("trusted");

        let cards = plugins_list(&state).expect("the list");
        let card = cards.first().expect("the installed plugin");
        assert!(card.state != "active");
        assert_eq!(card.state_reason.as_deref(), Some("this build has no plugin engine"));
    }

    #[test]
    fn enabling_and_disabling_move_the_plugin_in_and_out_of_the_set() {
        // ★ Enable is "activating" in the design's state machine, not a flag to be honoured at the next start. The
        // assertion is on the set because that is what decides whether a share can be answered.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        loadable(&state);
        state
            .db()
            .set_plugin_enabled(ID, false, testing::FIXED_NOW_MS)
            .expect("switched off");
        let id = PluginId::parse(ID).expect("an id");

        super::plugins_enable(&state, ID).expect("it enables and loads");
        assert!(state.plugin_set().expect("an engine").lock().contains(&id), "enable did not load it");

        super::plugins_disable(&state, ID).expect("it disables and unloads");
        assert!(!state.plugin_set().expect("an engine").lock().contains(&id), "disable did not unload it");
    }

    #[test]
    fn confirming_an_enabled_plugins_capabilities_loads_it_without_a_restart() {
        // ★ The consent flow can run after the switch: the user enables a plugin, is asked what it may do, and approves.
        // Before this, the set stayed empty until the next launch and the plugin looked broken for no stated reason.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        write_loadable_files(&state);
        state
            .db()
            .insert_plugin(ID, true, false, "installed", testing::FIXED_NOW_MS, testing::FIXED_NOW_MS)
            .expect("a plugin row");

        let card = super::plugins_consent_respond(&state, ID, &["log:*".to_owned()])
            .expect("the decision is recorded");
        assert_eq!(card.state, "active", "the card the consent returns shows the plugin already loaded");
        assert!(state.plugin_set().expect("an engine").lock().contains(&PluginId::parse(ID).expect("an id")));
    }

    #[test]
    fn uninstalling_takes_the_plugin_out_of_the_set() {
        // ★ A removed plugin whose compiled form stayed in the set could still answer a share, from an artifact the
        // user just deleted.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);
        loadable(&state);
        let summary = load_installed_plugins(
            &state,
            &synra_observe::crash::StartupVerdict::Clean,
            &synra_plugin_api::revocation::RevocationList::empty(),
        );
        assert_eq!(summary.len(), 1);
        let state = state.with_plugin_loads(summary);
        let id = PluginId::parse(ID).expect("an id");
        assert!(state.plugin_set().expect("an engine").lock().contains(&id));

        super::plugins_uninstall(&state, ID, false).expect("it is removed");
        assert!(!state.plugin_set().expect("an engine").lock().contains(&id), "the compiled form stayed behind");
        assert_eq!(state.plugin_loads().len(), 0, "the report still claims it is loaded");
    }

    #[test]
    fn the_cached_updates_are_readable_by_a_page_that_was_not_open() {
        // ★ The background pass runs whether or not the plugins page is open. This is how its answer reaches a page that
        // mounts later, sorted so two readers see the same order.
        let (_root, state) = crate::testing::file_state();
        for (plugin, version) in [("@acme/two", "3.0.0"), ("@acme/one", "2.0.0")] {
            let db = state.db();
            db.insert_plugin(plugin, false, false, "good", 1, 1).expect("plugin");
            db.record_plugin_version(plugin, "1.0.0", "wasm", "digest", "{}", 1).expect("version");
            db.save_plugin_update_check(&synra_store::PluginUpdateCheck {
                plugin_id: plugin.to_owned(),
                current_version: "1.0.0".to_owned(),
                checked_at_ms: 123,
                registry: None,
                available_version: Some(version.to_owned()),
                added_capabilities: Vec::new(),
            }).expect("check");
        }
        let restarted = crate::testing::reopen(&state);
        let updates = super::plugins_updates(&restarted).expect("saved updates");

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].plugin_id.as_str(), "@acme/one");
        assert_eq!(updates[0].version, "2.0.0");
        assert_eq!(updates[1].plugin_id.as_str(), "@acme/two");
    }

    #[test]
    fn an_update_announces_its_version_and_its_warnings() {
        // ★ The registry update path produced an `InstallReport` and then threw it away; the file-association path already
        // announced the same report. A warning about an unsigned package must not depend on which button started the
        // update, so both paths now send this notice.
        let state = crate::testing::state();
        let subscriber = state.events.subscribe();
        let report = synra_ipc::InstallReport {
            plugin_id: PluginId::parse(ID).expect("a plugin id"),
            version: "2.0.0".to_owned(),
            warnings: vec!["the package is not signed".to_owned()],
            enabled: false,
            publisher_id: None,
            fingerprint: None,
        };

        announce_updated(&state, &report);

        let delivery = state.events.take(subscriber).expect("one event");
        match delivery.events().first() {
            Some(synra_ipc::event::KernelEvent::Notice { message, params }) => {
                assert_eq!(message, "notice.updated");
                assert_eq!(params["pluginId"], ID);
                assert_eq!(params["version"], "2.0.0");
                assert_eq!(params["warnings"][0], "the package is not signed");
            }
            other => panic!("expected an update notice, got {other:?}"),
        }
    }
}
