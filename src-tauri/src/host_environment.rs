//! Build target facts shared by the main window and both plugin hosts.

use synra_plugin_host::HostKind;

pub(crate) const PLATFORM: &str = std::env::consts::OS;

pub(crate) fn kind() -> HostKind {
    if synra_core::platform_of_os(PLATFORM).is_mobile() {
        HostKind::Mobile
    } else {
        HostKind::Desktop
    }
}
