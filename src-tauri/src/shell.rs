//! The window: what it is built over, and what the interface may call.
//!
//! # Why this module is in the library and not in `main.rs`
//!
//! Because Android has no binary entry point to put it in. The generated `gen/android` project is a Kotlin activity
//! that loads a **shared library** and calls one C symbol, `start_app`; that symbol is produced by
//! `#[tauri::mobile_entry_point]` applied to a function in the library target. A `main.rs` is never linked into that
//! library, so `main.rs` is the one file on Android that cannot hold anything.
//!
//! The desktop binary keeps its own `main.rs`, and it keeps it small: it calls [`run_window`] here and turns the answer
//! into an exit code. So the two platforms share one assembly of the window and neither duplicates the other's.
//!
//! # Why it is behind the `tauri-shell` feature
//!
//! Because the library's other half must stay testable without a WebView. `cargo test --no-default-features` compiles
//! this crate with no `tauri` in the graph at all, and this module is the only thing that would break that — so it is
//! the only thing gated. The same reasoning as `build.rs`'s, and the two stay in step because both name the same
//! feature rather than each other.

use std::sync::Arc;

use parking_lot::Mutex;
use synra_ipc::DiagExportResult;
use synra_store::{Paths, SynraDb};
use tauri::Manager;

use crate::commands;
use crate::state::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// The wrappers
// ─────────────────────────────────────────────────────────────────────────────
//
// ★ Why each command is written twice.
//
// `#[tauri::command]` generates a module (`__cmd__identity_get`) and a symbol per command, and it does that by
// referring to `tauri::State`. The library's command bodies deliberately do not depend on `tauri` — the
// `--no-default-features` build runs them with no WebView in the build graph at all — so the bodies in `commands::*`
// are plain functions and the attribute has to be applied out here, in the one place that is only built when the shell
// is.
//
// The wrappers are as thin as the signature allows: take the arguments, call, return what came back. None of them
// decides anything, because a decision made here is a decision no test can reach.
//
// ★ Why the dispatch table is also in this module, and not next to the builder.
//
// Because `#[tauri::command]` on a `pub fn` writes its wrapper macro with `#[macro_export]`, which puts that macro at
// the crate root **instead of** leaving it in textual scope here. `generate_handler!` expands to an unqualified
// `__cmd__identity_get!(…)`, so it resolves only where the macro's textual scope still reaches — inside this module,
// below each definition. One module above, the same line fails with "cannot find macro". The alternative was forty
// explicit macro imports; this is one function.
//
// ★ `needless_pass_by_value` is switched off for this block, and it is the one place in the crate where that is right.
//
// Every argument here is owned because `#[tauri::command]` deserialises it that way — the macro writes the
// `Deserialize` bound into the generated `__cmd__*` module, so `&str` is not an option without changing what the
// command accepts over the wire. The lint is correct everywhere else in this repository and wrong in exactly this
// block, which is the distinction an `allow` is for.

#[allow(clippy::needless_pass_by_value)]
mod wrappers {
    use synra_ipc::{
        ConfigPatch, ConfigSnapshot, DevicesSnapshot, DiagExportOptions, IdentitySnapshot, IntentDecisionDto,
        IntentSnapshot, IntentSourceDto, InstallReport, InstallSourceDto, IpcError, PluginSnapshot, ScanOptions,
    };
    use synra_protocol::IntentId;

    use super::{AppState, DiagExportResult, commands};

    #[tauri::command]
    pub fn identity_get(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<IdentitySnapshot, IpcError> {
        commands::identity::identity_get(&state)
    }

    #[tauri::command]
    pub fn identity_set_display_name(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        name: String,
    ) -> Result<(), IpcError> {
        commands::identity::identity_set_display_name(&state, &name)
    }

    #[tauri::command]
    pub fn devices_snapshot(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<DevicesSnapshot, IpcError> {
        commands::devices::devices_snapshot(&state)
    }

    /// Starts a scan.
    ///
    /// ★ `(async)`, and it is not a style choice: the body waits out the protocol's scan window (up to 1.2 seconds of
    /// listening for answers), and a plain `#[tauri::command]` runs its body on the thread that received the invoke —
    /// which is the window's. Tauri's `(async)` on a **synchronous** function puts it on the sync thread pool instead
    /// (`sync_threadpool` in the macro's own vocabulary), so the window keeps painting while the network is asked. The
    /// daemon makes the same move for the same reason, by running discovery on a thread of its own.
    #[tauri::command(async)]
    pub fn devices_scan_start(state: tauri::State<'_, std::sync::Arc<AppState>>, opts: ScanOptions) -> Result<String, IpcError> {
        commands::devices::devices_scan_start(&state, &opts)
    }

    #[tauri::command]
    pub fn devices_scan_cancel(state: tauri::State<'_, std::sync::Arc<AppState>>, scan_id: String) -> Result<(), IpcError> {
        commands::devices::devices_scan_cancel(&state, &scan_id)
    }

    #[tauri::command]
    pub fn devices_forget(state: tauri::State<'_, std::sync::Arc<AppState>>, device_id: String) -> Result<(), IpcError> {
        commands::devices::devices_forget(&state, &device_id)
    }

    /// Asks a discovered device to pair, and records it when the other person accepts.
    #[tauri::command]
    pub async fn pairing_request(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        device_id: String,
    ) -> Result<(), IpcError> {
        let state = std::sync::Arc::clone(&state);
        super::blocking(move || commands::pairing::pairing_request(&state, &device_id)).await
    }

    /// Answers a pairing request that is waiting.
    #[tauri::command]
    pub fn pairing_respond(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        request_id: String,
        accept: bool,
    ) -> Result<(), IpcError> {
        commands::pairing::pairing_respond(&state, &request_id, accept)
    }

    /// Refuses a waiting pairing request. The link stays up, so the dialer may ask again.
    #[tauri::command]
    pub fn pairing_cancel(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        request_id: String,
    ) -> Result<(), IpcError> {
        commands::pairing::pairing_cancel(&state, &request_id)
    }

    #[tauri::command]
    pub fn plugins_list(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<Vec<PluginSnapshot>, IpcError> {
        commands::plugins::plugins_list(&state)
    }

    #[tauri::command]
    pub fn plugins_enable(state: tauri::State<'_, std::sync::Arc<AppState>>, plugin_id: String) -> Result<(), IpcError> {
        commands::plugins::plugins_enable(&state, &plugin_id)
    }

    #[tauri::command]
    pub fn plugins_disable(state: tauri::State<'_, std::sync::Arc<AppState>>, plugin_id: String) -> Result<(), IpcError> {
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

    /// Abandons a transfer, and refuses one that is not in flight.
    #[tauri::command]
    pub fn transfer_cancel(state: tauri::State<'_, std::sync::Arc<AppState>>, transfer_id: String) -> Result<(), IpcError> {
        commands::transfers::transfer_cancel(&state, &transfer_id)
    }

    /// Answers the prompt for an incoming transfer.
    ///
    /// The listener thread is waiting on this answer, and the file is accepted or refused when it arrives — so this is a
    /// decision about a peer's connection, not a preference stored for later.
    #[tauri::command]
    pub fn transfer_respond(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        transfer_id: String,
        accept: bool,
        remember: Option<bool>,
    ) -> Result<(), IpcError> {
        commands::transfers::transfer_respond(&state, &transfer_id, accept, remember)
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
        trust: String,
    ) -> Result<(), IpcError> {
        commands::plugins::plugins_publisher_decide(&state, &publisher_id, &trust)
    }

    /// Sends a file to a paired device, and answers the transfer it started.
    ///
    /// ★ **It answers now, not when the file has moved.** The dial and the block loop run on a worker thread that owns
    /// the transfer's row and its cancel token; what this returns is the identifier the panel needs in order to show the
    /// transfer, and to stop it. The refusals that can happen before a worker exists — an unknown device, a file that
    /// cannot be planned — are still errors here, because a caller that has a file but no thread must be told.
    #[tauri::command]
    pub fn transfer_send_file(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        request: synra_ipc::dto::SendFileRequest,
    ) -> Result<synra_protocol::ids::TransferId, IpcError> {
        commands::transfers::transfer_send_file_start(state.inner(), &request)
    }

    /// Sends text as a file, and answers the transfer it started.
    ///
    /// As [`transfer_send_file`](Self::transfer_send_file): the temporary file is written before the worker starts, and the
    /// worker removes it when it is done, whichever way it ends.
    #[tauri::command]
    pub fn transfer_send_text(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        request: synra_ipc::dto::SendTextRequest,
    ) -> Result<synra_protocol::ids::TransferId, IpcError> {
        commands::transfers::transfer_send_text_start(state.inner(), &request)
    }

    /// The transfers in flight.
    #[tauri::command]
    pub fn transfer_list(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
    ) -> Vec<synra_ipc::dto::TransferSnapshot> {
        commands::transfers::transfer_list(&state)
    }

    /// Records that a plugin page has drawn, after checking the session it was given.
    ///
    /// The label is the page's, not the plugin's: two pages of one plugin are two WebViews and two sessions, and a command
    /// that took a plugin id could not tell them apart.
    #[tauri::command]
    pub fn plugin_ui_ready(state: tauri::State<'_, std::sync::Arc<AppState>>, label: String) -> Result<(), IpcError> {
        commands::plugin_ui::plugin_ui_ready(&state, &label)
    }

    /// Answers one call from a plugin page.
    ///
    /// `(async)` because this is the one command whose operations may do I/O — a network fetch, a transfer — and the
    /// plugin page that called it is not the window that must keep painting.
    #[tauri::command(async)]
    pub fn plugin_ui_rpc(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        token: String,
        method: String,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        commands::plugin_ui::plugin_ui_rpc_arc(state.inner(), &token, &method, &args)
    }

    /// Opens a plugin's page in its own WebView, under its own origin.
    ///
    /// ★ The contract declares it now (`plugin_ui_open` => PluginUi, **Host**), which is what makes registering it legal:
    /// `registration.test.ts` refuses a shell that registers a name the kernel's table does not carry.
    /// # What is deliberately *not* here
    ///
    /// Every decision is in [`commands::plugin_ui::open_request`] — the address, the label, the token, the record in
    /// `hosted_ui` — because those are values a test can assert. What is left is the one call no test in this repository
    /// can make: `WebviewWindowBuilder`.
    #[tauri::command]
    pub fn plugin_ui_open(
        app: tauri::AppHandle,
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        plugin_id: String,
    ) -> Result<(), IpcError> {
        use tauri::Manager as _;

        // ★ One page per plugin: a second open is a focus, not a second window. Building another would issue a second
        // session for the same plugin and replace the first entry in `hosted_ui`, leaving the first page holding a token
        // no table names.
        let id = synra_protocol::ids::PluginId::parse(&plugin_id).map_err(|error| {
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}))
        })?;
        let label = commands::plugin_ui::window_label(&id);
        if let Some(window) = app.get_window(&label) {
            let _ = window.show();
            let _ = window.set_focus();
            return Ok(());
        }

        let request = match commands::plugin_ui::open_request(&state, &plugin_id)? {
            commands::plugin_ui::OpenDecision::AlreadyOpen => {
                if let Some(window) = app.get_window(&label) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                return Ok(());
            }
            commands::plugin_ui::OpenDecision::Open(request) => request,
        };
        let failed = |error: tauri::Error| {
            IpcError::unknown(synra_protocol::ErrorCode::Internal)
                .with_params(serde_json::json!({ "reason": error.to_string() }))
        };
        let url = request.url.parse::<tauri::Url>().map_err(|error| {
            IpcError::unknown(synra_protocol::ErrorCode::Internal)
                .with_params(serde_json::json!({ "reason": error.to_string() }))
        })?;
        // ★ The address the WebView actually received, not the string `open_request` built. A navigation candidate is a
        // parsed `Url` too, and comparing the two normalised forms is what makes the first load and a hash route pass
        // while every other destination is refused.
        let opened = url.as_str().to_owned();
        let window = tauri::WebviewWindowBuilder::new(&app, &request.label, tauri::WebviewUrl::CustomProtocol(url))
            .initialization_script(&request.script)
            // ★ The page may not navigate away. It holds this plugin's token and grants, and the script is attached to
            // the WebView rather than to an address — so another page, another origin or an external scheme would keep
            // this page's authority while showing something the opener never chose.
            .on_navigation(move |candidate| {
                commands::plugin_ui::stays_on_page(candidate.as_str(), &opened)
            })
            // And `window.open` is denied outright: a second window has no initialization script, so it would be a page
            // with no token — and on some platforms it would be a browser window the plugin opened, which is a decision
            // the platform capability exists to make.
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()
            .map_err(|error| {
                // The reservation `open_request` made must not outlive a window that was never built: otherwise every
                // failed open would leave the plugin permanently "already open" with no window to focus.
                commands::plugin_ui::forget_window(&state, &request.label);
                failed(error)
            })?;

        // ★ Closing the window ends the session: the token table is the page's authority, and an entry that outlives the
        // page would let a closed window keep answering. `Destroyed` rather than `CloseRequested` because a close can be
        // cancelled, and a cancelled close must keep the page it kept.
        let state = std::sync::Arc::clone(state.inner());
        let label = request.label.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                commands::plugin_ui::forget_window(&state, &label);
            }
        });
        Ok(())
    }

    /// Replaces an installed plugin with the newest version the registry publishes.
    #[tauri::command]
    pub fn plugins_update(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        plugin_id: String,
    ) -> Result<(), IpcError> {
        commands::plugins::plugins_update(&state, &plugin_id)
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

    // ★ The four that had no implementation until the orchestration moved out of the daemon binary.
    //
    // The reason recorded against them used to be "needs the intent board behind AppState", which was false: the board
    // was already in the state. What was missing was roughly 250 lines of routing that only a binary could reach.
    #[tauri::command]
    pub fn intent_dispatch(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        source: IntentSourceDto,
    ) -> Result<IntentId, IpcError> {
        commands::intent::intent_dispatch(&state, &source)
    }

    /// Every intent the board is holding, for the home page's "received" panel.
    ///
    /// No arguments, and that is the command's whole interface: it answers about the board rather than about one intent —
    /// which is the distinction `intent_snapshot` **below** it draws, and the one the interface missed while it called
    /// that command and read a list out of it.
    #[tauri::command]
    pub fn intent_list(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<Vec<IntentSnapshot>, IpcError> {
        commands::intent::intent_list(&state)
    }

    #[tauri::command]
    pub fn intent_snapshot(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        intent_id: IntentId,
    ) -> Result<IntentSnapshot, IpcError> {
        commands::intent::intent_snapshot(&state, intent_id)
    }

    #[tauri::command]
    pub fn intent_decide(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        intent_id: IntentId,
        decision: IntentDecisionDto,
    ) -> Result<(), IpcError> {
        commands::intent::intent_decide(&state, intent_id, &decision)
    }

    #[tauri::command]
    pub fn intent_cancel(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        intent_id: IntentId,
    ) -> Result<(), IpcError> {
        commands::intent::intent_cancel(&state, intent_id)
    }

    #[tauri::command]
    pub fn config_get(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<ConfigSnapshot, IpcError> {
        commands::config::config_get(&state)
    }

    #[tauri::command]
    pub fn config_set(state: tauri::State<'_, std::sync::Arc<AppState>>, patch: ConfigPatch) -> Result<(), IpcError> {
        commands::config::config_set(&state, &patch)
    }

    #[tauri::command]
    pub fn diag_export(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        options: DiagExportOptions,
    ) -> Result<DiagExportResult, IpcError> {
        commands::diag::diag_export(&state, &options)
    }

    /// Runs the startup probes and answers them.
    ///
    /// ★ No arguments: the questions are fixed and the answers are about this machine, not about what the caller wants
    /// to know. A filter parameter would let a page hide a failing probe by not asking for it.
    #[tauri::command]
    pub fn diag_probes(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
    ) -> Result<synra_ipc::ProbeReportDto, IpcError> {
        commands::diag::diag_probes(&state)
    }

    /// The tail of the shell's own log.
    ///
    /// `lines: Option<u32>` rather than an options struct, because there is one option. A struct for a single count
    /// would be an IPC type that has to be mirrored in the TypeScript contract for no shape of its own.
    #[tauri::command]
    pub fn diag_logs_tail(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
        lines: Option<u32>,
    ) -> Result<Vec<String>, IpcError> {
        commands::diag::diag_logs_tail(&state, lines)
    }

    /// What the shell's metrics hold.
    ///
    /// ★ No arguments, and that is the command's whole interface.
    ///
    /// Metrics are read as a set rather than queried one at a time; a command that took a metric name would be a command
    /// whose caller already knew what it was looking for, which is the opposite of what a diagnostic is for.
    #[tauri::command]
    pub fn diag_metrics(
        state: tauri::State<'_, std::sync::Arc<AppState>>,
    ) -> Result<synra_observe::metrics::MetricsSnapshot, IpcError> {
        commands::diag::diag_metrics(&state)
    }

    /// The dispatch table, built where the macros above are still in scope.
    ///
    /// The return type is written out rather than left to inference because the caller is
    /// [`tauri::Builder::invoke_handler`], whose bound is what makes the shape a fact rather than a guess.
    pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
        tauri::generate_handler![
            identity_get,
            identity_set_display_name,
            devices_snapshot,
            devices_scan_start,
            devices_scan_cancel,
            devices_forget,
            pairing_request,
            pairing_respond,
            pairing_cancel,
            plugins_list,
            plugins_enable,
            plugins_disable,
            plugins_consent_respond,
            plugins_install,
            plugins_preview,
            plugins_catalog_search,
            plugins_request_registry_search,
            plugins_check_update,
            plugins_update,
            plugins_reload,
            plugins_uninstall,
            plugin_ui_open,
            plugin_ui_ready,
            plugin_ui_rpc,
            transfer_cancel,
            transfer_respond,
            transfer_send_file,
            transfer_send_text,
            transfer_list,
            plugins_audit,
            plugins_audit_clear,
            plugins_sync_plan,
            plugins_sync_run,
            plugins_publishers,
            plugins_publisher_decide,
            intent_dispatch,
            intent_list,
            intent_snapshot,
            intent_decide,
            intent_cancel,
            config_get,
            config_set,
            diag_export,
            diag_probes,
            diag_logs_tail,
            diag_metrics,
        ]
    }
}

/// The shell, with the state it reads and the commands it answers, and no window opened yet.
///
/// Split out of [`run_window`] so the two platforms can differ in how they *enter* the window — a desktop process
/// returns an exit code, an Android activity calls in through JNI and has nowhere to return one to — while sharing how
/// the window is built.
pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        // ★ A plugin page is served from its own origin, and this is the only place that knows how: the address is split
        // against the ids the host actually opened (`hosted_ui`), and the file comes from that plugin's bundle through the
        // same path rules `ui_file` tests. Everything a request cannot have — another plugin's files, `..`, a missing
        // bundle — is a refusal here rather than a guess.
        .register_uri_scheme_protocol("synra-plugin", |ctx, request| {
            crate::commands::plugin_ui::serve(ctx.app_handle(), &request)
        })
        .setup(|app| {
            // ★ The single-instance check, **before** the state opens the database or binds a listener. A second launch
            // has nothing to do here: it has already written the file that asks the running instance to come forward.
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("the data directory could not be found: {error}"))?;
            let mut instance_request = None;
            match crate::instance::claim(&data_dir) {
                Ok(crate::instance::Start::First(guard)) => {
                    instance_request = Some(guard.request_path().to_path_buf());
                    // Managed, not dropped: the operating-system lock lives as long as this value does, and this value
                    // lives as long as the application.
                    app.manage(guard);
                }
                Ok(crate::instance::Start::Second) => {
                    // ★ The running instance has been asked to show itself; this process has nothing to do. Returning
                    // before `app_state` is the point: a second database and a second listener port must not exist.
                    app.handle().exit(0);
                    return Ok(());
                }
                Err(error) => {
                    // ★ A lock we cannot take is not a reason to refuse to run. A read-only data directory would
                    // otherwise make the window unopenable, and two instances is the lesser fault.
                    eprintln!("synra: the single-instance lock could not be taken: {error}");
                }
            }
            let state = app_state(app.handle())?;
            // ★ The event bridge, started before the state is moved into the window. It holds a clone of the bus, so a
            // command that publishes later finds a subscriber waiting — a bridge started after the first command would
            // miss exactly the event that command raised.
            let bridge_cancel = synra_core::supervisor::Cancellation::new();
            crate::events::spawn(
                app.handle().clone(),
                std::sync::Arc::clone(&state.events),
                &bridge_cancel,
            );
            app.manage(std::sync::Arc::new(state));
            // The bridge and the pairing watcher are the two threads that outlive the window but own no socket; their
            // tokens go in one value so the shutdown does not have to remember a second place.
            let pairing_cancel = synra_core::supervisor::Cancellation::new();
            app.manage(Background {
                cancels: vec![bridge_cancel, pairing_cancel.clone()],
            });
            // ★ The listener starts after the state is managed, because it reads that state to build its host and router. The token
            // is managed rather than dropped so a later shutdown can stop the loop: without it the thread lives until the process
            // does, and a restart would fail to bind a port the old one still holds.
            app.manage(crate::listener::spawn(app.handle()));
            // ★ Starts the thread that turns a request on the desk into an event the interface can draw. Without it a peer
            // could ask to pair and the window would look at it and never say so.
            crate::commands::pairing::watch(app.handle().clone(), &pairing_cancel);
            // ★ The tray, and it is reported rather than fatal: the window is the application, and a desktop without an
            // indicator host (some Linux setups) must still get one. What it buys is the second way back to a window and
            // a **deliberate** way out — see `run_window`, where closing the window hides it instead of ending the process.
            if let Err(error) = crate::tray::install(app.handle()) {
                eprintln!("synra: the tray icon could not be installed: {error}");
            }
            // ★ Last, because a request honoured before there is a window would find nothing to show.
            if let Some(request) = instance_request {
                crate::instance::watch(app.handle().clone(), request);
            }
            // ★ The window starts hidden (`visible: false` in `tauri.conf.json`) and is shown here, after the
            // single-instance check. A second launch would otherwise create and flash a window before deciding it has
            // nothing to do.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
            Ok(())
        })
        .invoke_handler(wrappers::handler())
}

/// Builds and runs the window, reporting what stopped it rather than unwinding.
///
/// # Why closing the window hides it
///
/// Because this shell is a kernel: a listener on a port, transfers in flight, a plugin set loaded. Closing a window is how
/// a desktop application is put away, not how one is ended, and the two are not the same gesture for a program that other
/// devices can reach. The tray's **Quit** is the deliberate exit; the close button asks the window to go away.
///
/// # Errors
///
/// A message saying what stopped it.
pub fn run_window() -> Result<(), String> {
    let app = builder()
        .build(tauri::generate_context!())
        .map_err(|error| format!("the window could not be opened: {error}"))?;
    app.run(|app, event| {
        // ★ Every deliberate exit passes here, including the tray's Quit: asking the background workers to stop before
        // the process ends is what closes the listener's port and lets the current discovery turn finish its write.
        if let tauri::RunEvent::ExitRequested { .. } = &event {
            stop_background(app);
        }
        if let tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } = event
            && label == "main"
        {
            // ★ Before the hide, not after: the request is the window's decision, and hiding a window whose close was
            // not prevented would be hiding one that is already going.
            api.prevent_close();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    });
    Ok(())
}

/// Asks every worker that outlives the window to stop.
///
/// # Why one function
///
/// Because a shutdown that lists its workers by hand is one that forgets the next worker somebody starts. The listener
/// and discovery are the two that own a socket; both are asked here, and a new one belongs in this list with them.
/// The stop tokens for the threads that outlive the window but own no socket.
///
/// The event bridge and the pairing watcher both live for the process; they are not the listener, whose token is managed
/// separately because it is created after this value, and not discovery, whose token lives in `AppState`.
struct Background {
    cancels: Vec<synra_core::supervisor::Cancellation>,
}

fn stop_background(app: &tauri::AppHandle) {
    // The listener's token is managed rather than held by `AppState`, because it is created after the state is managed.
    if let Some(managed) = app.try_state::<Option<synra_core::supervisor::Cancellation>>()
        && let Some(cancel) = managed.inner().as_ref()
    {
        cancel.cancel();
    }
    if let Some(background) = app.try_state::<Background>() {
        for cancel in &background.cancels {
            cancel.cancel();
        }
    }
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>()
        && let Some(discovery) = state.discovery.as_ref()
    {
        discovery.stop();
    }
}

/// The entry point Android calls, and only there.
///
/// ★ **Why this is `#[cfg(mobile)]` rather than `#[cfg_attr(mobile, …)]`.**
///
/// With `cfg_attr` the attribute is skipped on a desktop build and what is left is an ordinary `pub fn`
/// that nothing calls — a function that exists so that a reader can see it and wonder. Gated on `mobile`,
/// it is **absent** from a desktop build, which is the honest description: a desktop process has `main`,
/// and this symbol is what a shared library offers instead of one.
///
/// The refusal is printed rather than returned because there is no caller to return it to: a mobile entry
/// point returns no exit code, and a panic across the FFI boundary would abort the process with nothing in
/// the log.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    if let Err(reason) = run_window() {
        eprintln!("synra: {reason}");
    }
}

/// The state, over the real database in the real place.
///
/// # Errors
///
/// A message saying what stopped it. The three ways this can fail are a directory that cannot be created, a database
/// that cannot be opened or migrated, and an identity that cannot be written — and each one is a fact about the user's
/// machine rather than a bug in the code, so each is reported in words rather than as an opaque error code.
/// Runs a blocking command body off the window's thread, and awaits it.
///
/// # Why a helper rather than `spawn_blocking` at each call site
///
/// Because the error when the task cannot be started is not something a caller can act on, and every call site would invent
/// its own sentence for it. One sentence, one place: a blocked thread that could not be obtained is the runtime shutting down,
/// which during a command means the application is closing.
async fn blocking<T, F>(body: F) -> Result<T, synra_ipc::IpcError>
where
    F: FnOnce() -> Result<T, synra_ipc::IpcError> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(body).await {
        Ok(answer) => answer,
        Err(_) => Err(synra_ipc::IpcError::unknown(synra_protocol::ErrorCode::Internal)
            .with_params(serde_json::json!({"reason": "the runtime stopped before the task ran"}))),
    }
}

fn app_state(app: &tauri::AppHandle) -> Result<AppState, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("the data directory could not be found: {error}"))?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("the cache directory could not be found: {error}"))?;

    let paths = Paths::new(data_dir, cache_dir);
    paths
        .ensure_directories()
        .map_err(|error| format!("the data directories could not be created: {error}"))?;

    // ★ The crash pipeline starts before anything that can panic.
    //
    // A report of what went wrong has to be written by the layer that is already working, so the hook goes in before the
    // database is opened and before any plugin is compiled. The daemon installs the same hook at the same point; without
    // it here, a plugin that kills the window leaves no report, the verdict below can never see a crash loop, and safe
    // mode — the only way back to the settings screen a crashing plugin has made unreachable — never triggers.
    let crashes = synra_observe::crash::CrashStore::new(paths.log_dir());
    synra_observe::crash::install_panic_hook(crashes.clone(), env!("CARGO_PKG_VERSION"));
    // The verdict reads what previous runs left behind. It is taken once, here, and passed to the loader: a second read
    // later would see this run's own startup and answer a different question.
    let verdict = crashes.verdict(now_ms());

    let mut db = SynraDb::open(&paths.db_path())
        .map_err(|error| format!("the database could not be opened: {error}"))?;
    // Migrated on the way in, so a shell that starts on an older database does not fail every command with a message
    // about a missing table.
    db.migrate().map_err(|error| format!("the database could not be migrated: {error}"))?;

    // ★ The platform string the kernel stores, taken from the build target rather than from `std::env::consts`.
    //
    // They usually agree, and they disagree in the cross-compiled case — and the identity records the platform at first
    // launch, so a wrong answer here is written to disk and then believed by every peer for the life of the install.
    //
    // ★ Android is the case that made this matter.
    //
    // `std::env::consts::OS` answered `"android"` for this build all along; what it did not answer was a string the
    // kernel's own vocabulary has a word for. `identity` stores what a peer is told, and a peer that is told "unknown"
    // records a device it can never name again — so the arm exists and says "android".
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let state = AppState::new(db, paths, platform, now_ms())
        .map_err(|error| format!("the identity could not be prepared: {error:?}"))?;

    let mut state = with_plugins_or_refuse(state, &verdict);
    // ★ Bind the listener **before** discovery, so the announcement can carry the port a peer should dial. The order is the
    // whole fix: announcing the discovery port meant every pairing dialled the responder, which accepts datagrams and not links.
    state = crate::listener::bind(state);
    // ★ And the half the shell never had: its own discovery. Started before the log so a bind failure is reported through
    // the log if one is running, and its failure is not fatal — see the function.
    state = with_discovery_or_report(state);
    // ★ The shell's own log, started before anything else can go wrong — and its failure is reported rather than fatal.
    // A window that refuses to open because it cannot write a diagnostic file trades the user's actual work for its
    // record, and `diag_export` says the section is absent instead of claiming there was nothing to log.
    if let Err(error) = state.start_logging() {
        eprintln!("synra-app: logging is not running: {error}");
    }

    Ok(state)
}

/// The window's discovery worker, or a window without one.
///
/// # Errors
///
/// None, and for the same reason [`with_plugins_or_refuse`] has none: a machine with no network, a firewall and a sandbox
/// all produce the same bind failure, and none of them is a reason to lose the window — this device simply cannot be
/// *found*, and every other command still works.
///
/// ★ What it costs is the **scan**, and what that looks like to a user is worth writing down: `devices_scan_start` refuses
/// a network scan with the reason, and the interface falls back to the cached read — the devices already known are still
/// listed. A window that refused to open over a busy UDP port would trade the user's whole application for one button.
fn with_discovery_or_report(state: AppState) -> AppState {
    // ★ The announced port is the discovery port, and that is not a mistake — it is the only port this process binds. See
    // `crate::discovery`'s header: this build has no transport, and the wire format refuses `port: 0`.
    let announcement = state.announcement(synra_protocol::limits::DISCOVERY_PORT);
    // The announcement the worker makes: it reports an identifier, and the host builds the card and publishes it.
    let announce: crate::discovery::Announce = {
        let db = Arc::clone(&state.db);
        let host = state.host.clone();
        let events = Arc::clone(&state.events);
        Arc::new(move |device_id: &str| {
            if let Ok(Some(card)) = crate::commands::devices::card_for(&db, &host, device_id) {
                events.emit(synra_ipc::event::KernelEvent::DeviceCardUpdated(Box::new(card)));
            }
        })
    };
    let started = crate::discovery::Discovery::start(        announcement,
        synra_protocol::limits::DISCOVERY_PORT,
        Arc::clone(&state.db),
        state.host.clone(),
        state.started_at_ms,
        announce,
    );

    match started {
        Ok(discovery) => {
            match discovery.responder_address() {
                Ok(address) => println!("synra: discovery is listening on {address}"),
                Err(error) => eprintln!("synra: {error}"),
            }
            state.with_discovery(discovery)
        }
        Err(reason) => {
            eprintln!("synra: discovery is not running: {reason}");
            state
        }
    }
}

/// The plugin set when an engine starts, and a shell without one when it does not.
///
/// # Errors
///
/// None. This is the one failure in start-up that does not stop the shell, and that is the whole point of the function.
///
/// ★ An engine that will not start — a missing system library, a CPU feature this build was not compiled for — is a shell
/// that still shows devices, settings and diagnostics. Dropping the window over it would turn a recoverable condition
/// into an unusable application, and the plugin half is the smaller loss.
///
/// ★ This is also what keeps an Android build a *shell* rather than a black screen when the engine cannot start: the
/// `wasmtime` engine is the largest and most platform-sensitive dependency in the tree, and this function is the seam
/// that lets the rest of the application survive it.
fn with_plugins_or_refuse(
    state: AppState,
    verdict: &synra_observe::crash::StartupVerdict,
) -> AppState {
    // ★ Before the engine, because it is about what is on disk rather than what can run. A directory an author copied
    // into this shell's data directory becomes a row here — disabled and untrusted, so the consent screen is still the
    // gate — and the list can show it even in a build with no engine at all.
    let _ = commands::plugins::discover_plugins(&state, state.started_at_ms);
    // ★ The engine the platform wants, asked for rather than assumed.
    //
    // `BackendKind::default_for` is the table the kernel keeps for exactly this question, and a shell that named
    // `WasmtimeCranelift` itself would be a second answer to it — one that is wrong on a phone, where a JIT costs build
    // time and a baseline compiler is the compromise the design document names.
    let host_kind = if cfg!(any(target_os = "android", target_os = "ios")) {
        synra_plugin_host::HostKind::Mobile
    } else {
        synra_plugin_host::HostKind::Desktop
    };
    let kind = synra_plugin_host::BackendKind::default_for(host_kind);
    match synra_plugin_host::engine::WasmtimeBackend::new(kind) {
        Ok(engine) => {
            // ★ The host, which this shell had never supplied.
            //
            // `PluginSet::new` took an engine alone when this line was written and takes a host as well now. The second
            // argument is not a detail: an instance with no session refuses every host call, which is what made the
            // capability gate inert, and requiring the host is what made that state unrepresentable.
            let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
                Arc::clone(&state.db),
                Arc::clone(&state.platform),
            ));
            let backend = synra_plugin_host::Backend::new(Arc::new(engine));
            // ★ The same question the daemon asks, for the same reason: an engine that cannot enforce what this
            // host will ask of it must not run third-party code at all.
            //
            // ★ And here the answer costs the **plugin half only**, which is the shape this function already has: the window
            // still shows devices, settings and diagnostics. An engine missing a limit joins an engine that would not start,
            // rather than becoming an engine that runs plugins without the bound.
            match synra_plugin_host::engine::check_enforceable(
                &backend,
                &synra_plugin_host::quota::Quota::default(),
            ) {
                Ok(()) => {
                    // ★ And the audit gets its durable half, which the shell never had.
                    //
                    // The daemon has installed this sink since the table's writer was written; the shell did not, so a
                    // plugin's audit trail here lived in the in-memory ring and **died with the process** — while the
                    // migration that created `plugin_audit` says what the table is for: *"the durable part is what makes the
                    // interface's 'recent activity' survive a restart."*
                    let set = synra_core::dispatch::PluginSet::new(backend, host)
                        .with_audit_sink(crate::plugin_host::audit_sink(Arc::clone(&state.db)))
                        // ★ And the same recorder the `diag_metrics` command reads, so the counts a plugin's
                        // capability requests produce are reachable from the window rather than only from a log line.
                        .with_metrics(Arc::clone(&state.metrics));
                    // ★ The set is built, and then it is **filled**. Until this line the shell created an empty set and
                    // never called the loader, so every plugin was listed, could be enabled, and was never askable —
                    // `intent_dispatch` matched nothing and `life_state` called it `unavailable` forever.
                    let state = state.with_plugins(Arc::new(Mutex::new(set)));
                    let revocations = commands::plugins::revocations(&state);
                    let summary = commands::plugins::load_installed_plugins(&state, verdict, &revocations);
                    state.with_plugin_loads(summary)
                }
                Err(error) => {
                    // The same shape as the arm below: said once, and the shell carries on without the plugin half.
                    eprintln!(
                        "synra: plugins will not run in this build: this engine cannot enforce the limits this shell sets: {error}"
                    );
                    state.with_engine_refusal(format!(
                        "this build cannot enforce the limits a plugin needs: {error}"
                    ))
                }
            }
        }
        Err(error) => {
            // ★ Said once, here, and then carried by every command that needs a plugin.
            //
            // `AppState::plugin_set` is where the user-facing refusal is produced, with the reason attached. Saying it
            // again per command would be two statements of one fact, and the second is the one nobody reads.
            eprintln!("synra: plugins will not run in this build: {error}");
            state.with_engine_refusal(format!("this build has no plugin engine: {error}"))
        }
    }
}

/// The clock, in milliseconds since the epoch.
///
/// A clock before 1970 reads as zero rather than refusing: refusing to open a window over it would help nobody, and the
/// value only has to be monotonic within this process to be useful in a log line.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}
