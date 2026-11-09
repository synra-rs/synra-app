use super::*;

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
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("plugins_installed", []),
        installed,
    );
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("plugins_active", []),
        active,
    );

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
            IpcError::unknown(synra_protocol::ErrorCode::Internal).with_params(
                serde_json::json!({"pluginId": row.plugin_id, "detail": error.to_string()}),
            )
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
pub(crate) fn set_enabled(
    state: &AppState,
    plugin_id: &str,
    enabled: bool,
) -> Result<(), IpcError> {
    let id = parse(plugin_id)?;
    if !state
        .db()
        .set_plugin_enabled(&id.to_string(), enabled, state.started_at_ms)
        .map_err(internal)?
    {
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
pub(crate) fn announce(state: &AppState, plugin_id: &PluginId) {
    let Ok(rows) = state.db().plugins() else {
        return;
    };
    let Some(row) = rows
        .into_iter()
        .find(|row| row.plugin_id == plugin_id.to_string())
    else {
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
pub(crate) fn snapshot(
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
        match set
            .lifecycle(plugin_id)
            .map(synra_plugin_host::Lifecycle::state)
        {
            Some(synra_plugin_host::State::Faulted { reason, .. }) => Some(reason.clone()),
            _ => None,
        }
    });
    let decisions = state.db().plugin_grants(&row.plugin_id).unwrap_or_default();
    // ★ One read of the load report for both the state word and its sentence. The lock is taken here, the strings are
    // cloned out, and neither helper needs the guard — a second acquisition under the first would be a deadlock.
    let (refused, safe_mode) = {
        let loads = state.plugin_loads();
        (
            loads.refusal_for(&row.plugin_id).map(str::to_owned),
            loads.safe_mode.clone(),
        )
    };
    let state_word = life_state(
        row,
        is_loaded,
        fault_reason.is_some(),
        manifest.is_some(),
        refused.is_some(),
    );
    let update_check = state
        .db()
        .plugin_update_check(&row.plugin_id)
        .map_err(internal)?;
    let update = update_check
        .as_ref()
        .and_then(crate::commands::plugin_updates::info)
        .transpose()?;

    Ok(PluginSnapshot {
        plugin_id: plugin_id.clone(),
        // ★ The same function the removal uses, so the path in the dialog is the directory that will be deleted.
        data_path: state
            .paths
            .plugin_sandbox_dir(&row.plugin_id)
            .display()
            .to_string(),
        title: manifest
            .as_ref()
            .map_or_else(|| row.plugin_id.clone(), |manifest| manifest.title.clone()),
        description: manifest
            .as_ref()
            .and_then(|manifest| manifest.description.clone()),
        version: manifest
            .as_ref()
            .map_or_else(String::new, |manifest| manifest.version.clone()),
        publisher: manifest
            .as_ref()
            .map_or_else(String::new, |manifest| manifest.publisher.id.clone()),
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
        has_ui: manifest
            .as_ref()
            .is_some_and(|manifest| manifest.runtime.ui.is_some()),
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
            .map(|manifest| {
                manifest
                    .capabilities
                    .iter()
                    .map(|declared| declared.cap.clone())
                    .collect()
            })
            .unwrap_or_default(),
        // ★ Granted and denied are both listed, and both are sorted.
        //
        // Listing only the granted ones makes a refusal invisible, which is how a user ends up re-granting something
        // they deliberately refused — and being unable to see what they refused is what makes them do it.
        granted: decisions
            .iter()
            .filter(|d| d.granted)
            .map(|d| d.capability.clone())
            .collect(),
        denied: decisions
            .iter()
            .filter(|d| !d.granted)
            .map(|d| d.capability.clone())
            .collect(),
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
pub(crate) fn life_state(
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
pub(crate) fn read_manifest_text(state: &AppState, plugin_id: &str) -> Option<String> {
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
