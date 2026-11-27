//! Prepare automatic replacements, migrate the old instance and commit with policy checked under the store lock.

use synra_ipc::{InstallReport, IpcError};
use synra_protocol::PluginId;
use synra_store::{PluginUpdateCheck, SynraDb};

use crate::state::AppState;

/// Prepares an automatic update before migration, preserving the verified bytes through the lifecycle switch.
pub(crate) fn update_directory(
    state: &AppState,
    check: &PluginUpdateCheck,
    path: &std::path::Path,
    policy: synra_plugin_api::update::UpdatePolicy,
    cancel: &synra_core::supervisor::Cancellation,
) -> Result<InstallReport, IpcError> {
    let plugin_id = &check.plugin_id;
    let version = check.available_version.as_deref().ok_or_else(automatic_cancelled)?;
    ensure_automatic_check(&state.db(), check, policy, cancel)?;
    let id = PluginId::parse(plugin_id).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason":error.to_string()}))
    })?;
    let prepared = {
        let plugins = state.plugin_set()?;
        let set = plugins.lock();
        synra_core::install::prepare_automatic_update(
            &state.db(),
            set.backend(),
            &state.paths.plugins_dir(),
            path,
            synra_store::sqlite::now_ms(),
            synra_plugin_host::verify::HostFacts::desktop(),
            policy,
        )
        .map_err(|error| install_error(&error))?
    };
    if prepared.plugin_id() != plugin_id || prepared.version() != version {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
                serde_json::json!({
                    "reason": "the verified package does not match the checked plugin and version",
                }),
            ),
        );
    }
    if let Some(reason) = super::super::plugins::revocations(state).revokes(plugin_id, version) {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"stage":"update-policy", "reason":reason})),
        );
    }
    ensure_automatic_check(&state.db(), check, policy, cancel)?;
    super::super::plugins::migrate_and_unload(state, &id, Some(prepared.version()))?;
    let outcome = (|| {
        let plugins = state.plugin_set()?;
        let _set = plugins.lock();
        let mut db = state.db();
        ensure_automatic_check(&db, check, policy, cancel)?;
        prepared
            .install(&mut db, check, synra_store::sqlite::now_ms())
            .map_err(|error| install_error(&error))
    })();
    let report = super::super::plugins::complete_update(state, &id, outcome)?;
    super::super::plugins::announce_updated(state, &report);
    Ok(report)
}

pub(crate) fn ensure_automatic_check(
    db: &SynraDb,
    check: &PluginUpdateCheck,
    policy: synra_plugin_api::update::UpdatePolicy,
    cancel: &synra_core::supervisor::Cancellation,
) -> Result<(), IpcError> {
    let current = synra_core::plugin_updates::automatic_check_is_current(db, check, policy, cancel)
        .map_err(crate::state::internal)?;
    if !current {
        return Err(automatic_cancelled());
    }
    Ok(())
}

fn automatic_cancelled() -> IpcError {
    IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
        serde_json::json!({"reason":"the automatic update was cancelled or its check or policy changed"}),
    )
}

fn install_error(refusal: &synra_core::install::InstallRefusal) -> IpcError {
    IpcError::unknown(refusal.code())
        .with_params(serde_json::json!({"stage":refusal.stage, "reason":refusal.reason}))
}
