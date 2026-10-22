//! Prepare automatic replacements, migrate the old instance and commit with policy checked under the store lock.

use synra_ipc::{InstallReport, IpcError};
use synra_protocol::PluginId;

use crate::state::AppState;

/// Prepares an automatic update before migration, preserving the verified bytes through the lifecycle switch.
pub(crate) fn update_directory(
    state: &AppState,
    plugin_id: &str,
    version: &str,
    path: &std::path::Path,
    registry: Option<&str>,
    policy: synra_plugin_api::update::UpdatePolicy,
    cancel: &synra_core::supervisor::Cancellation,
) -> Result<InstallReport, IpcError> {
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
    ensure_automatic_policy(state, registry, policy, cancel)?;
    super::super::plugins::migrate_and_unload(state, &id, Some(prepared.version()))?;
    let outcome = ensure_automatic_policy(state, registry, policy, cancel).and_then(|()| {
        let plugins = state.plugin_set()?;
        let set = plugins.lock();
        let mut db = state.db();
        let result = if cancel.is_cancelled()
            || super::configured_policy(&db) != Some(policy)
            || !same_registry(&db, registry)
        {
            Err(automatic_cancelled())
        } else {
            prepared
                .install(&mut db, synra_store::sqlite::now_ms())
                .map_err(|error| install_error(&error))
        };
        drop(db);
        drop(set);
        result
    });
    let report = super::super::plugins::complete_update(state, &id, outcome)?;
    super::super::plugins::announce_updated(state, &report);
    Ok(report)
}

fn ensure_automatic_policy(
    state: &AppState,
    registry: Option<&str>,
    policy: synra_plugin_api::update::UpdatePolicy,
    cancel: &synra_core::supervisor::Cancellation,
) -> Result<(), IpcError> {
    if cancel.is_cancelled()
        || super::policy(state) != Some(policy)
        || !same_registry(&state.db(), registry)
    {
        return Err(automatic_cancelled());
    }
    Ok(())
}

fn same_registry(db: &synra_store::SynraDb, expected: Option<&str>) -> bool {
    synra_core::plugin_updates::registry_setting(db)
        .is_ok_and(|current| current.as_deref() == expected)
}

fn automatic_cancelled() -> IpcError {
    IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
        serde_json::json!({"reason":"the automatic update was cancelled or its configuration changed"}),
    )
}

fn install_error(refusal: &synra_core::install::InstallRefusal) -> IpcError {
    IpcError::unknown(refusal.code())
        .with_params(serde_json::json!({"stage":refusal.stage, "reason":refusal.reason}))
}
