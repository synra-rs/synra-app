//! Enforce the shared command interface before any Tauri app-command handler executes.

use synra_ipc::IpcError;
use synra_ipc::command::{CommandRegistry, Interface};
use synra_protocol::ErrorCode;

pub(crate) fn authorize(webview_label: &str, command: &str) -> Result<(), IpcError> {
    let descriptor =
        CommandRegistry::find(command).ok_or_else(|| IpcError::unknown(ErrorCode::Unsupported))?;
    let caller = if webview_label == "main" {
        Interface::Host
    } else {
        Interface::Plugin
    };
    if descriptor.callable_by(caller) {
        Ok(())
    } else {
        Err(IpcError::unknown(ErrorCode::CapabilityDenied))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_command_is_denied_to_other_webviews_before_execution() {
        for descriptor in CommandRegistry::for_interface(Interface::Host) {
            assert!(
                authorize("main", descriptor.name).is_ok(),
                "{}",
                descriptor.name
            );
            for label in ["plugin-example", "main-copy", ""] {
                assert_eq!(
                    authorize(label, descriptor.name).unwrap_err().code,
                    ErrorCode::CapabilityDenied,
                    "{}",
                    descriptor.name
                );
            }
        }
    }

    #[test]
    fn only_registered_plugin_entry_points_are_available_outside_the_main_webview() {
        for name in ["plugin_ui_ready", "plugin_ui_rpc"] {
            assert!(authorize("plugin-example", name).is_ok());
            assert_eq!(
                authorize("main", name).unwrap_err().code,
                ErrorCode::CapabilityDenied
            );
        }
        assert_eq!(
            authorize("plugin-example", "identity_force_repair")
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
}
