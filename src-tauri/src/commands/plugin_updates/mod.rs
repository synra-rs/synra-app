//! Checking the registry for updates without a person pressing a button.
//!
//! A daily registry pass with bounded retries after failed installs. Notify-only is the default; an explicit automatic policy admits verified replacements
//! from the same trusted signer with no added capabilities, through the kernel's prepared update pipeline.
//!
//! # Why the answer is one notice rather than one per plugin
//!
//! Because the next step is the same for all of them — open the plugins page and look — and a device with eight outdated
//! plugins would otherwise stack eight banners that say the same thing.
//!
//! # Why it can be switched off
//!
//! Because a background request to a registry is traffic the user did not ask for at that moment. The setting is
//! `plugins.auto_update`; absent means notify-only and `never` stops background checks. Automatic installation requires
//! selecting auto-minor or auto-all on the settings page.

use std::sync::Arc;
use std::time::Duration;

use synra_core::supervisor::Cancellation;
use synra_ipc::IpcError;
use synra_ipc::UpdateInfo;
use synra_ipc::event::KernelEvent;
use synra_plugin_api::update::UpdatePolicy;
use synra_protocol::{ErrorCode, PluginId};
use synra_store::PluginUpdateCheck;

use crate::state::AppState;

mod apply;
pub(super) use apply::update_directory as apply_automatic_update;
pub use synra_core::plugin_updates::AUTO_UPDATE_KEY;
pub(super) use synra_core::plugin_updates::configured_policy;
#[cfg(test)]
use synra_core::plugin_updates::{CHECK_EVERY_MS, due};
use synra_core::plugin_updates::{due_plugins, record_failure};

/// How long one sleep slice is.
///
/// Not 250 ms: the wait between checks is a day, and waking four times a second for it would be a busy loop that costs
/// more than the check. Five minutes is short enough that turning the switch off takes effect in this cycle rather than
/// tomorrow.
const SLICE: Duration = synra_core::plugin_updates::POLL_INTERVAL;

/// Whether the background check is on.
///
/// Absent means on: the design's default is a check every 24 hours, and a setting a user has never seen should not be the
/// reason nothing is ever checked. A value that cannot be read also means on, because the failure mode of "off" is
/// silence about updates and the failure mode of "on" is one request a day.
#[must_use]
pub fn enabled(state: &AppState) -> bool {
    policy(state).is_some()
}

/// The selected update policy; `None` disables background checks. An absent or corrupt setting only permits notices.
#[must_use]
pub fn policy(state: &AppState) -> Option<UpdatePolicy> {
    configured_policy(&state.db())
}

/// Asks the registry about installed versions due for their daily check or a bounded install retry.
///
/// # Why per-plugin errors are swallowed
///
/// Because this is a courtesy, not a command: no registry configured, a registry that is down, and a plugin the registry
/// no longer publishes all look the same from here — nothing to report for that plugin — and making the interface explain
/// three network facts on a page the user did not open would be worse than silence. The manual check on the plugins page
/// is where a refusal is named.
#[must_use]
pub fn check_all(state: &AppState, now_ms: i64, cancel: &Cancellation) -> Vec<UpdateInfo> {
    let plugins = match due_plugins(&state.db(), now_ms) {
        Ok(plugins) => plugins,
        Err(error) => {
            eprintln!("synra: plugin update schedule could not be read: {error}");
            return Vec::new();
        }
    };
    let mut updates = Vec::new();
    for plugin in plugins {
        if cancel.is_cancelled() || !enabled(state) {
            break;
        }
        if let Ok(Some(info)) = super::registry::check_update_at(state, &plugin, now_ms) {
            if try_automatic(state, &info, now_ms, cancel) {
                continue;
            }
            updates.push(info);
        }
    }
    updates
}

fn try_automatic(state: &AppState, info: &UpdateInfo, now_ms: i64, cancel: &Cancellation) -> bool {
    let Some(selected) = policy(state).filter(|policy| *policy != UpdatePolicy::NotifyOnly) else {
        return false;
    };
    if info.needs_new_consent || cancel.is_cancelled() {
        return false;
    }
    let check = state.db().plugin_update_check(info.plugin_id.as_str());
    let check = match check {
        Ok(Some(check))
            if check.checked_at_ms == now_ms
                && check.available_version.as_deref() == Some(info.version.as_str()) =>
        {
            check
        }
        Ok(_) => return false,
        Err(error) => {
            eprintln!("synra: automatic update check could not be read: {error}");
            return false;
        }
    };
    match super::registry::update_automatically(
        state,
        info.plugin_id.as_str(),
        &info.version,
        selected,
        cancel,
    ) {
        Ok(_) => true,
        Err(error) => {
            let recorded = record_failure(&state.db(), &check, selected, cancel, now_ms);
            eprintln!(
                "synra: automatic update of {} failed: {error:?}; retry state: {recorded:?}",
                info.plugin_id
            );
            false
        }
    }
}

/// Converts a saved positive result to the IPC shape; negative checks have no update to offer.
pub(super) fn info(check: &PluginUpdateCheck) -> Option<Result<UpdateInfo, IpcError>> {
    check.available_version.as_ref().map(|version| {
        let plugin_id = PluginId::parse(&check.plugin_id).map_err(|error| {
            IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"detail": error.to_string()}))
        })?;
        Ok(UpdateInfo {
            plugin_id,
            version: version.clone(),
            needs_new_consent: !check.added_capabilities.is_empty(),
            added_capabilities: check.added_capabilities.clone(),
        })
    })
}

/// Saves the answer before publishing its card, including the removal of a previously advertised update.
pub(super) fn record(
    state: &AppState,
    check: &PluginUpdateCheck,
) -> Result<Option<UpdateInfo>, IpcError> {
    let saved = state
        .db()
        .save_plugin_update_check(check)
        .map_err(|error| {
            IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"detail": error.to_string()}))
        })?;
    if !saved {
        return Err(
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "pluginId": check.plugin_id,
                "reason": "the installed version or registry changed while checking for updates",
            })),
        );
    }
    let result = info(check).transpose()?;
    let plugin_id = PluginId::parse(&check.plugin_id).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    super::plugins::announce(state, &plugin_id);
    Ok(result)
}

/// Tells the interface how many updates the pass found.
pub fn announce(state: &AppState, updates: &[UpdateInfo]) {
    if updates.is_empty() {
        return;
    }
    state.publish(KernelEvent::Notice {
        message: "notice.updates".to_owned(),
        params: serde_json::json!({
            "count": updates.len(),
            "plugins": updates.iter().map(|info| info.plugin_id.as_str()).collect::<Vec<_>>(),
        }),
    });
}

/// The loop: check due versions every five minutes, preserving successful checks across restarts.
///
/// # Why the wait is walked in five-minute steps
///
/// Because the setting can change while the loop is asleep, and a single sleep for a day would make "off" mean "off from
/// tomorrow". The shared schedule permits daily checks and bounded retries after failed automatic installs.
pub fn run(state: &Arc<AppState>, cancel: &Cancellation) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let now = synra_store::sqlite::now_ms();
        if enabled(state) {
            let updates = check_all(state, now, cancel);
            if !updates.is_empty() {
                eprintln!(
                    "synra: {} plugin update(s) available: {}",
                    updates.len(),
                    updates
                        .iter()
                        .map(|info| info.plugin_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                announce(state, &updates);
            }
        }
        if cancel.wait(SLICE) {
            return;
        }
    }
}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod tests;
