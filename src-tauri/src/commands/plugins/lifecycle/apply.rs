use super::*;

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
pub(crate) fn apply_lifecycle(state: &AppState, id: &PluginId) -> Result<(), String> {
    let Ok(plugins) = state.plugin_set() else {
        // No engine: the row is the user's decision and the card explains why nothing runs. A command that failed here
        // would make "the plugin is on but this build cannot run it" look like the switch did not take.
        return Ok(());
    };
    let should_run = {
        let db = state.db();
        let rows = db
            .plugins()
            .map_err(|error| format!("the plugin rows could not be read: {error}"))?;
        // The database lock is released before the set is taken: the two locks are never held in one order here, so
        // there is no second order for another command to deadlock on.
        drop(db);
        rows.into_iter()
            .find(|row| row.plugin_id == id.as_str())
            .is_some_and(|row| row.should_run())
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
    if faulted
        && set
            .clear_fault(
                id,
                u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0),
            )
            .unwrap_or(false)
    {
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
        synra_core::plugin::load_plugin_with_config(
            &mut set,
            &state.db,
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
pub(crate) fn apply_unload(state: &AppState, id: &PluginId) {
    if let Ok(plugins) = state.plugin_set() {
        // ★ `deactivate` first: it is the plugin's one chance to release what it holds, and the loader that started it
        // is what has to stop it. The set's lock is released by the guard leaving this block, before the report's lock.
        let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0);
        synra_core::plugin::deactivate(&mut plugins.lock(), id, now);
    }
    state.plugin_loads.lock().record_unloaded(id);
}
