//! Checking the registry for updates without a person pressing a button.
//!
//! A daily registry pass. Notify-only is the default; an explicit automatic policy also admits verified replacements
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
#[cfg(test)]
use synra_core::plugin_updates::CHECK_EVERY_MS;
pub(super) use synra_core::plugin_updates::configured_policy;
use synra_core::plugin_updates::due;

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

/// Asks the registry about installed versions whose successful check is at least a day old.
///
/// # Why per-plugin errors are swallowed
///
/// Because this is a courtesy, not a command: no registry configured, a registry that is down, and a plugin the registry
/// no longer publishes all look the same from here — nothing to report for that plugin — and making the interface explain
/// three network facts on a page the user did not open would be worse than silence. The manual check on the plugins page
/// is where a refusal is named.
#[must_use]
pub fn check_all(state: &AppState, now_ms: i64, cancel: &Cancellation) -> Vec<UpdateInfo> {
    let Ok(rows) = state.db().plugins() else {
        return Vec::new();
    };
    let Ok(checks) = state.db().plugin_update_checks() else {
        return Vec::new();
    };
    let mut updates = Vec::new();
    for row in rows {
        if cancel.is_cancelled() || !enabled(state) {
            break;
        }
        let last_ms = checks
            .iter()
            .find(|check| check.plugin_id == row.plugin_id)
            .map(|check| check.checked_at_ms);
        if !due(last_ms, now_ms) {
            continue;
        }
        if let Ok(Some(info)) = crate::commands::registry::check_update(state, &row.plugin_id) {
            if !info.needs_new_consent
                && let Some(selected) = policy(state)
                && selected != UpdatePolicy::NotifyOnly
                && !cancel.is_cancelled()
                && super::registry::update_automatically(
                    state,
                    &row.plugin_id,
                    &info.version,
                    selected,
                    cancel,
                )
                .is_ok()
            {
                continue;
            }
            updates.push(info);
        }
    }
    updates
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
/// tomorrow". The steps do not make the check more frequent: [`due`] still allows one every 24 hours.
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
mod tests {
    use super::*;

    fn update(plugin: &str) -> UpdateInfo {
        UpdateInfo {
            plugin_id: synra_protocol::PluginId::parse(plugin).expect("a plugin id"),
            version: "2.0.0".to_owned(),
            needs_new_consent: false,
            added_capabilities: Vec::new(),
        }
    }

    #[test]
    fn a_device_that_never_touched_the_setting_checks_for_updates() {
        let state = crate::testing::state();
        assert!(
            enabled(&state),
            "absent means on, which is the design's 24-hour default"
        );

        state
            .db()
            .set_config(
                AUTO_UPDATE_KEY,
                &serde_json::json!("never"),
                synra_store::sqlite::now_ms(),
            )
            .expect("the setting is written");
        assert!(!enabled(&state), "the switch says off");
    }

    #[test]
    fn an_update_pass_sends_one_notice_naming_how_many() {
        // ★ One notice, not one per plugin: the next step is the same for all of them, and a stack of identical banners is
        // what a person learns to close without reading.
        let state = crate::testing::state();
        let subscriber = state.events.subscribe();
        let updates = [update("@acme/one"), update("@acme/two")];

        announce(&state, &updates);

        let delivery = state.events.take(subscriber).expect("one event");
        match delivery.events().first() {
            Some(KernelEvent::Notice { message, params }) => {
                assert_eq!(message, "notice.updates");
                assert_eq!(params["count"], 2);
                assert_eq!(params["plugins"][0], "@acme/one");
            }
            other => panic!("expected an updates notice, got {other:?}"),
        }
    }

    #[test]
    fn a_check_is_due_once_a_day_and_after_never() {
        // ★ The bound is on the check, not on the sleep: the loop wakes every five minutes so a setting change takes
        // effect, and this is what keeps those wakes from becoming registry requests.
        assert!(due(None, 0), "a device that has never checked is due");
        assert!(!due(Some(1_000), 1_000 + CHECK_EVERY_MS - 1), "not yet");
        assert!(
            due(Some(1_000), 1_000 + CHECK_EVERY_MS),
            "the day has passed"
        );
        assert!(
            due(Some(10_000), 1_000),
            "a clock that went backwards is due, not silent"
        );
    }

    #[test]
    fn a_saved_check_publishes_its_card_and_a_negative_check_clears_it() {
        let state = crate::testing::state();
        let plugin = "@acme/hello";
        state
            .db()
            .insert_plugin(plugin, false, false, "good", 1, 1)
            .expect("plugin");
        state
            .db()
            .record_plugin_version(&synra_store::PluginVersionRecord { plugin_id: plugin, version: "1.0.0", artifact: "wasm", digest: "digest", manifest: "{}", installed_ms: 1, ..Default::default() })
            .expect("version");
        let subscriber = state.events.subscribe();
        let mut check = PluginUpdateCheck {
            plugin_id: plugin.to_owned(),
            current_version: "1.0.0".to_owned(),
            checked_at_ms: 123,
            registry: None,
            available_version: Some("2.0.0".to_owned()),
            added_capabilities: vec!["log:*".to_owned()],
        };
        assert!(record(&state, &check).expect("positive result").is_some());
        let delivery = state.events.take(subscriber).expect("card event");
        let Some(KernelEvent::PluginUpdated(card)) = delivery.events().first() else {
            panic!("expected a card event");
        };
        assert_eq!(
            card.update.as_ref().map(|info| info.version.as_str()),
            Some("2.0.0")
        );
        assert_eq!(card.update_checked_at_ms, Some(123));
        check.available_version = None;
        check.added_capabilities.clear();
        check.checked_at_ms = 456;
        assert!(record(&state, &check).expect("negative result").is_none());
        let delivery = state.events.take(subscriber).expect("card event");
        let Some(KernelEvent::PluginUpdated(card)) = delivery.events().first() else {
            panic!("expected a card event");
        };
        assert!(card.update.is_none());
        assert_eq!(card.update_checked_at_ms, Some(456));
    }
}
