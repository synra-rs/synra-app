use crate::commands;
use crate::state::AppState;
use tauri::Manager;

/// Installs or updates the `.synra` package at `package`, then brings the window forward.
///
/// # Why one function
///
/// Three operating-system entries lead here — a command-line argument, a drag-and-drop, and macOS's "open document" event
/// — and all three have to make the same install/update/already-installed decision. A copy per entry would drift, and the
/// version comparison is exactly the kind of thing that drifts silently.
pub(super) fn open_package(app: &tauri::AppHandle, package: &std::path::Path) {
    let state = app.state::<std::sync::Arc<AppState>>();
    match synra_core::install::package_manifest(package) {
        Ok(manifest) => {
            let id = manifest.id.as_str();
            let active = state.db().active_plugin(id).ok().flatten();
            let action = crate::launch::action(
                active.as_ref().map(|row| row.version.as_str()),
                &manifest.version,
            );
            match action {
                crate::launch::PackageAction::Install => {
                    let source = synra_ipc::InstallSourceDto::Path {
                        path: package.display().to_string(),
                    };
                    match commands::plugins::plugins_install(&state, &source) {
                        Ok(report) => {
                            state.publish(synra_ipc::event::KernelEvent::Notice {
                                message: "notice.installed".to_owned(),
                                params: serde_json::json!({
                                    "pluginId": report.plugin_id,
                                    "version": report.version,
                                    "warnings": report.warnings,
                                }),
                            });
                        }
                        Err(error) => {
                            eprintln!(
                                "synra: {} could not be installed: {error:?}",
                                package.display()
                            );
                        }
                    }
                }
                crate::launch::PackageAction::Update => {
                    match commands::plugins::update_directory(&state, id, package) {
                        Ok(report) => {
                            state.publish(synra_ipc::event::KernelEvent::Notice {
                                message: "notice.updated".to_owned(),
                                params: serde_json::json!({
                                    "pluginId": id,
                                    "version": report.version,
                                    "warnings": report.warnings,
                                }),
                            });
                        }
                        Err(error) => {
                            eprintln!(
                                "synra: {} could not be updated: {error:?}",
                                package.display()
                            );
                        }
                    }
                }
                crate::launch::PackageAction::AlreadyInstalled => {
                    state.publish(synra_ipc::event::KernelEvent::Notice {
                        message: "notice.alreadyInstalled".to_owned(),
                        params: serde_json::json!({
                            "pluginId": id,
                            "version": manifest.version,
                        }),
                    });
                }
                crate::launch::PackageAction::NewerInstalled => {
                    // ★ A double-clicked older package is not an instruction to downgrade. The window still comes
                    // forward, and the notice says why nothing changed.
                    state.publish(synra_ipc::event::KernelEvent::Notice {
                        message: "notice.newerInstalled".to_owned(),
                        params: serde_json::json!({
                            "pluginId": id,
                            "installed": active.as_ref().map_or("", |row| row.version.as_str()),
                            "offered": manifest.version,
                        }),
                    });
                }
            }
        }
        Err(error) => {
            eprintln!(
                "synra: {} is not a readable package: {error:?}",
                package.display()
            );
        }
    }
    // Desktop only: there is no hidden window to bring back on Android, and `window::show_main` is not compiled there.
    #[cfg(desktop)]
    crate::window::show_main(app);
    #[cfg(not(desktop))]
    let _ = app;
}
