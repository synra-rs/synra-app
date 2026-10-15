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

use synra_ipc::{IpcError, PluginSnapshot};
use synra_plugin_api::Manifest;
use synra_protocol::PluginId;

use crate::state::{AppState, internal};

/// Every plugin this device has installed, whether or not it is running.
///
/// # Errors
///
/// [`IpcError::Internal`] when the records cannot be read. Not an empty list: "nothing is installed" and "the database
/// could not be read" are different facts and the interface draws them differently.
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
    let path = state.paths.plugin_dir(plugin_id).join("manifest.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
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
