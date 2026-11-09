mod config_diag;
mod devices;
mod intent;
mod plugin_ui;
mod plugins;
mod transfers;

pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let allowed = crate::ipc_access::authorize(
            invoke.message.webview_ref().label(),
            invoke.message.command(),
        );
        if let Err(error) = allowed {
            invoke.resolver.reject(error);
            return true;
        }

        let dispatch = match invoke.message.command() {
            "identity_get"
            | "identity_set_display_name"
            | "devices_snapshot"
            | "devices_scan_start"
            | "devices_scan_cancel"
            | "devices_forget"
            | "devices_pair"
            | "devices_pairing_respond"
            | "devices_pairing_cancel"
            | "devices_pairing_snapshot"
            | "devices_input_snapshot"
            | "devices_input_respond"
            | "devices_input_stop" => devices::dispatch,
            "plugins_list"
            | "plugins_enable"
            | "plugins_disable"
            | "plugins_consent_respond"
            | "plugins_install"
            | "plugins_preview"
            | "plugins_catalog_search"
            | "plugins_request_registry_search"
            | "plugins_updates"
            | "plugins_check_update"
            | "plugins_update"
            | "plugins_reload"
            | "plugins_uninstall"
            | "plugins_audit"
            | "plugins_audit_clear"
            | "plugins_sync_plan"
            | "plugins_sync_run"
            | "plugins_publishers"
            | "plugins_publisher_decide" => plugins::dispatch,
            "transfer_cancel" | "transfer_respond" | "transfer_send_file"
            | "transfer_send_text" | "transfer_list" | "transfer_received" | "transfer_open"
            | "transfer_reveal" | "transfer_discard" => transfers::dispatch,
            "plugin_ui_open" | "plugin_ui_ready" | "plugin_ui_rpc" => plugin_ui::dispatch,
            "intent_dispatch" | "intent_list" | "intent_snapshot" | "intent_decide"
            | "intent_cancel" => intent::dispatch,
            "config_get" | "config_set" | "diag_export" | "diag_probes" | "diag_logs_tail"
            | "diag_crash_dismiss" | "diag_crashes" | "diag_metrics" => config_diag::dispatch,
            _ => return false,
        };
        dispatch(invoke)
    }
}
