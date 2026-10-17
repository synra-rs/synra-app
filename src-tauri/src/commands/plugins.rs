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

use synra_ipc::{AuditEntryDto, InstallReport, InstallSourceDto, IpcError, PluginSnapshot};
use synra_plugin_api::{Manifest, manifest_digest};
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
/// **Every capability the manifest declares must have been granted.** A plugin that is missing one cannot do what it
/// says it does, and a plugin in that state that ran anyway is precisely the silent degradation consent exists to
/// prevent.
///
/// ★ This is stricter than the design document implies, and deliberately so. `15-plugin-sdk.md` describes the manifest
/// declaration as `{ cap: "..." }` "supporting optionality", and `CapabilityDeclaration` has no `optional` field — only
/// `cap` and `reason`. So there is no author statement that a plugin degrades gracefully without something, and a rule
/// that guessed which capabilities were droppable would be inventing one. If optionality is added, this is where it
/// belongs.
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

    let decisions: Vec<(String, bool)> = manifest
        .capabilities
        .iter()
        .map(|declared| (declared.cap.clone(), granted.contains(&declared.cap)))
        .collect();

    // ★ Trusted only when nothing declared was declined.
    let trusted = decisions.iter().all(|(_, granted)| *granted);

    // The digest of the document the user is agreeing to, so a later manifest that declares more is visibly a different
    // document from the one these rows were decided against.
    let digest = manifest_digest(&text);

    state
        .db()
        .record_grants(&id.to_string(), &decisions, state.started_at_ms, &digest)
        .map_err(internal)?;

    if !state.db().set_plugin_trusted(&id.to_string(), trusted, state.started_at_ms).map_err(internal)? {
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
    Ok(snapshot(state, &row, &id, loaded.as_deref()))
}

/// Every plugin this device has installed, whether or not it is running.
///
/// # Errors
///
/// [`IpcError::Internal`] when the records cannot be read. Not an empty list: "nothing is installed" and "the database
/// could not be read" are different facts and the interface draws them differently.
/// Installs a plugin from a directory, through the checks, and reports what the user has to be told.
///
/// # Why the shell installs rather than the kernel
///
/// Because the pipeline needs an engine, and the kernel holds the seam rather than an engine. `synra_core::install` is
/// the pipeline — the same one the daemon runs, so the order of the checks has **one** implementation — and what differs
/// between the two hosts is the [`HostFacts`] passed in: a desktop can open a window, so a plugin that declares a
/// notification installs here without the warning a daemon earns.
///
/// # What an install is not
///
/// It is not an approval. The row this writes is installed, switched off and untrusted, and the answer says `enabled:
/// false` so that a caller cannot read the absence of a decision as a decision.
///
/// # Errors
///
/// * `Unsupported` for a registry source — this build has no fetcher, and refusing it by name is the honest alternative
///   to a network error that does not exist. It is also `Unsupported` when this shell has no plugin engine at all.
/// * `InvalidArgument` when a check refused the package; the params carry the stage and the reason.
pub fn plugins_install(state: &AppState, source: &InstallSourceDto) -> Result<InstallReport, IpcError> {
    let InstallSourceDto::Path { path } = source else {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({ "why": "this build has no registry client" })));
    };
    let plugins = state.plugin_set()?;
    let set = plugins.lock();
    // The database guard is scoped to the call rather than held while the report is built: this mutex is not reentrant,
    // and a `plugins_list` inside the same breadth would be a hang rather than an error.
    let outcome = {
        let db = state.db();
        synra_core::install::from_directory(
            &db,
            set.backend(),
            &state.paths.plugins_dir(),
            std::path::Path::new(path),
            synra_store::sqlite::now_ms(),
            synra_plugin_host::verify::HostFacts::desktop(),
        )
    };
    let installed = outcome.map_err(|refusal| {
        IpcError::unknown(refusal.code()).with_params(serde_json::json!({
            "stage": refusal.stage,
            "reason": refusal.reason,
        }))
    })?;
    Ok(InstallReport {
        plugin_id: parse(&installed.plugin_id)?,
        version: installed.version,
        warnings: installed.warnings,
        enabled: false,
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
    Ok(())
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
        // ★ `None` rather than a zeroed reading.
        //
        // The host has not observed this plugin yet, so it has no usage to report. A snapshot full of zeros would be
        // indistinguishable from a plugin that genuinely never uses anything — and those two need different reactions
        // from a user deciding whether to keep it.
        health: None,
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
fn read_manifest(state: &AppState, plugin_id: &str) -> Option<Manifest> {
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
fn parse(plugin_id: &str) -> Result<PluginId, IpcError> {
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
    use super::{entry_card, plugins_audit, plugins_consent_respond, plugins_install, plugins_list};
    use synra_ipc::InstallSourceDto;
    use synra_protocol::PluginId;
    use crate::testing;

    const ID: &str = "@synra-plugin/chat";

    /// A state with a scripted engine, because installing needs one and the fixture builds a shell without.
    ///
    /// The backend answers from a table rather than compiling Wasm: what is being tested is the shell's command — the
    /// source it accepts, the report it returns and the row that lands — and the pipeline behind it has its own tests.
    fn with_engine(state: crate::state::AppState) -> crate::state::AppState {
        let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::new(std::sync::Arc::clone(&state.db)));
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
    fn a_registry_source_is_refused_by_name() {
        // Refused rather than reported as a network failure: there is no fetcher, and "could not reach the registry"
        // would send a user to check a connection that was never used.
        let (_root, state) = testing::file_state();
        let state = with_engine(state);

        let refused = plugins_install(
            &state,
            &InstallSourceDto::Registry {
                plugin_id: PluginId::try_from("@acme/hello".to_owned()).expect("a plugin id"),
                version: None,
            },
        )
        .expect_err("there is no registry client");

        assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
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
    fn declining_a_capability_leaves_the_plugin_untrusted_and_the_refusal_visible() {
        // ★ A declined capability is recorded as a **denial**, not omitted. A plugin whose manifest declares two
        // capabilities and whose table holds one row is one the interface cannot describe: the user would be shown a
        // capability they never answered about, which is how they end up re-granting something they refused.
        let (_root, state) = installed();

        let card = plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

        assert_eq!(card.state, "awaiting_consent", "a declined capability means it cannot do what it claims");
        assert_eq!(card.granted, vec!["log:*".to_owned()]);
        assert_eq!(card.denied, vec!["device:list".to_owned()]);
        assert_eq!(
            grants(&state),
            vec![("device:list".to_owned(), false), ("log:*".to_owned(), true)],
        );
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
}
