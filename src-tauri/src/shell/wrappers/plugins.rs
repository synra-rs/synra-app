#![allow(clippy::needless_pass_by_value)]

use synra_ipc::{InstallReport, InstallSourceDto, IpcError, PluginSnapshot};

use crate::{commands, state::AppState};

#[tauri::command]
pub fn plugins_list(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<PluginSnapshot>, IpcError> {
    commands::plugins::plugins_list(&state)
}

#[tauri::command]
pub fn plugins_enable(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<(), IpcError> {
    commands::plugins::plugins_enable(&state, &plugin_id)
}

#[tauri::command]
pub fn plugins_disable(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<(), IpcError> {
    commands::plugins::plugins_disable(&state, &plugin_id)
}

/// Records the user's decision about what a plugin may do.
#[tauri::command]
pub fn plugins_consent_respond(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
    granted: Vec<String>,
) -> Result<PluginSnapshot, IpcError> {
    commands::plugins::plugins_consent_respond(&state, &plugin_id, &granted)
}

/// Removes a plugin, and its data only when the user asked for that.
///
/// `remove_data` is a parameter rather than a default here: the sentence the user confirms says whether their data
/// stays, and a default in this layer could disagree with it.
#[tauri::command]
pub fn plugins_uninstall(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
    remove_data: bool,
) -> Result<synra_ipc::RemoveReport, IpcError> {
    commands::plugins::plugins_uninstall(&state, &plugin_id, remove_data)
}

/// Lists the publishers whose signatures this device has seen.
#[tauri::command]
pub fn plugins_publishers(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<synra_ipc::dto::PublisherDto>, IpcError> {
    commands::plugins::plugins_publishers(&state)
}

/// Trusts, revokes, or un-decides a publisher's key.
#[tauri::command]
pub fn plugins_publisher_decide(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    publisher_id: String,
    fingerprint: String,
    trust: String,
) -> Result<(), IpcError> {
    commands::plugins::plugins_publisher_decide(&state, &publisher_id, &fingerprint, &trust)
}

/// Replaces an installed plugin with the newest version the registry publishes.
#[tauri::command]
pub fn plugins_update(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<InstallReport, IpcError> {
    commands::plugins::plugins_update(&state, &plugin_id)
}

/// The updates the last background pass found, so a page opened later can show them without asking the registry.
#[tauri::command]
pub fn plugins_updates(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<synra_ipc::UpdateInfo>, IpcError> {
    commands::plugins::plugins_updates(&state)
}

/// Tries a faulted plugin's load again without changing its switches.
#[tauri::command]
pub fn plugins_reload(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<PluginSnapshot, IpcError> {
    commands::plugins::plugins_reload(&state, &plugin_id)
}

/// Asks the registry whether a newer version is published for an installed plugin.
#[tauri::command]
pub fn plugins_check_update(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<Option<synra_ipc::dto::UpdateInfo>, IpcError> {
    commands::plugins::plugins_check_update(&state, &plugin_id)
}

/// Searches the configured registry for plugins.
#[tauri::command]
pub fn plugins_catalog_search(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    query: synra_ipc::dto::CatalogQuery,
) -> Result<Vec<synra_ipc::dto::CatalogItem>, IpcError> {
    commands::plugins::plugins_catalog_search(&state, &query)
}

/// Searches the registry with only the text, as the contract's simpler entry names it.
///
/// The endpoint is the same one [`plugins_catalog_search`] reaches; this command exists so a caller that has no paging
/// or sort to state does not have to invent them. Registering it is what makes the contract's name true.
#[tauri::command]
pub fn plugins_request_registry_search(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    query: String,
) -> Result<Vec<synra_ipc::dto::CatalogItem>, IpcError> {
    commands::plugins::plugins_catalog_search(
        &state,
        &synra_ipc::dto::CatalogQuery {
            query,
            registry: None,
            limit: None,
            offset: None,
            sort: None,
        },
    )
}

/// Describes a registry package before any of it is downloaded.
#[tauri::command]
pub fn plugins_preview(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
    version: Option<String>,
) -> Result<synra_ipc::dto::InstallPreviewDto, IpcError> {
    commands::plugins::plugins_preview(&state, &plugin_id, version.as_deref())
}

#[tauri::command]
pub fn plugins_install(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    source: InstallSourceDto,
) -> Result<InstallReport, IpcError> {
    commands::plugins::plugins_install(&state, &source)
}

// ★ The audit trail, which had no writer until a plugin instance was given a session.
//
// `offset` is optional so an existing caller that only knows `limit` keeps working: the first page is the default.
#[tauri::command]
pub fn plugins_audit(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: synra_protocol::PluginId,
    limit: u32,
    offset: Option<u32>,
) -> Result<Vec<synra_ipc::AuditEntryDto>, IpcError> {
    commands::plugins::plugins_audit(&state, &plugin_id, limit, offset.unwrap_or(0))
}

/// Forgets one plugin's audit history.
///
/// ★ It takes no `limit` and answers nothing: clearing is not a read with a side effect. The two answers a caller has
/// to tell apart are "cleared" and "this build cannot run plugins", and the second is the only failure.
#[tauri::command]
pub fn plugins_audit_clear(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: synra_protocol::PluginId,
) -> Result<(), IpcError> {
    commands::plugins::plugins_audit_clear(&state, &plugin_id)
}

/// Asks each paired device what it holds, and answers what would have to be sent.
///
/// ★ `(async)`, because the asking is one dial per device: the body runs on the sync thread pool rather than on the
/// thread that received the invoke, so the window keeps painting while the peers answer. Same attribute and same
/// reason as `devices_scan_start`.
#[tauri::command(async)]
pub fn plugins_sync_plan(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<synra_ipc::dto::SyncPlanDto, IpcError> {
    commands::sync_plan::plugins_sync_plan(&state, &plugin_id)
}

/// Sends the artifact a plan marked as needed, and reports what each device answered.
///
/// ★ `(async)` for the same reason as the plan: one dial and one transfer per device. A receiver that says it did not
/// install the package becomes an entry in the report rather than an error, so one unreachable device does not stop
/// the others.
#[tauri::command(async)]
pub fn plugins_sync_run(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plan: synra_ipc::dto::SyncPlanDto,
) -> Result<synra_ipc::dto::SyncReport, IpcError> {
    commands::sync_run::plugins_sync_run(&state, &plan)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        plugins_list,
        plugins_enable,
        plugins_disable,
        plugins_consent_respond,
        plugins_install,
        plugins_preview,
        plugins_catalog_search,
        plugins_request_registry_search,
        plugins_updates,
        plugins_check_update,
        plugins_update,
        plugins_reload,
        plugins_uninstall,
        plugins_audit,
        plugins_audit_clear,
        plugins_sync_plan,
        plugins_sync_run,
        plugins_publishers,
        plugins_publisher_decide,
    ];
    dispatch(invoke)
}
