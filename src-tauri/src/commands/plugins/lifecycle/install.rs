#[cfg(feature = "tauri-shell")]
use super::*;

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
    let backend = state
        .plugins
        .as_ref()
        .map(|set| set.lock().backend().clone());
    let Some(backend) = backend else {
        return Err("this shell has no plugin engine, so it cannot install a package".to_owned());
    };
    let replace = synra_core::install::package_manifest(archive)
        .ok()
        .and_then(|manifest| db.plugin_is_installed(manifest.id.as_str()).ok())
        .unwrap_or(false);
    let outcome = if replace {
        synra_core::install::update_from_package(
            db,
            &backend,
            &state.paths.plugins_dir(),
            archive,
            synra_store::sqlite::now_ms(),
            synra_plugin_host::verify::HostFacts::desktop(),
        )
    } else {
        synra_core::install::from_package(
            db,
            &backend,
            &state.paths.plugins_dir(),
            archive,
            synra_store::sqlite::now_ms(),
            synra_plugin_host::verify::HostFacts::desktop(),
        )
    };
    outcome.map_err(|refusal| format!("{}: {}", refusal.stage, refusal.reason))
}

/// Reconciles the live plugin runtime with a package installed by device sync.
///
/// A transfer writes the database and plugin directory on a listener thread. The loaded runtime must be replaced in the
/// same pass, otherwise it can keep answering with the old artifact while the database already reflects the new consent.
#[cfg(feature = "tauri-shell")]
pub(crate) fn reconcile_after_install(state: &AppState, plugin_id: &str) -> Result<(), String> {
    let id = parse(plugin_id).map_err(|_| "the synced plugin id is invalid".to_owned())?;
    apply_lifecycle(state, &id)
}
