use super::*;

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
                eprintln!(
                    "synra-app: {} was not recorded as a plugin: {problem}",
                    discovered.plugin_id
                );
            }
            None => eprintln!(
                "synra-app: {} was found and recorded as installed, switched off",
                discovered.plugin_id
            ),
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
    // Snapshot once for this pass; the shared loader holds no store guard while it executes guest code.
    let activation = synra_core::plugin::activation_config(set.host().as_ref());
    let summary = {
        synra_core::plugin::load_installed_with_config(
            &mut set,
            &state.db,
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
        eprintln!(
            "synra-app: {} was not loaded: {}",
            refused.plugin_id, refused.reason
        );
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
                eprintln!(
                    "synra: {} revocation entries read from {}",
                    list.len(),
                    path.display()
                );
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
