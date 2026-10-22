//! Checking the registry for updates without a person pressing a button.
//!
//! # What this is, and what it deliberately is not
//!
//! A background pass that asks the registry once a day whether anything installed has a newer version, and tells the
//! interface when something does. It **never installs**: 16 §6 lets an update proceed on its own only when the trust level
//! is high enough, the capabilities do not grow and the publisher has not changed; none of those checks are made here, so
//! the honest scope is "notice". Installing silently is a later decision, not a side effect of a check.
//!
//! # Why the answer is one notice rather than one per plugin
//!
//! Because the next step is the same for all of them — open the plugins page and look — and a device with eight outdated
//! plugins would otherwise stack eight banners that say the same thing.
//!
//! # Why it can be switched off
//!
//! Because a background request to a registry is traffic the user did not ask for at that moment. The setting is
//! `plugins.auto_update`; absent means **on**, which is the design's default of a check every 24 hours, and `false` is the
//! switch on the settings page.

use std::sync::Arc;
use std::time::Duration;

use synra_core::supervisor::Cancellation;
use synra_ipc::UpdateInfo;
use synra_ipc::event::KernelEvent;

use crate::state::AppState;

/// The configuration key that turns the background check on or off.
pub const AUTO_UPDATE_KEY: &str = "plugins.auto_update";

/// How often the registry is asked.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long one sleep slice is.
///
/// Not 250 ms: the wait is a day, and waking four times a second for it would be a busy loop that costs more than the
/// check. A five-minute slice notices a shutdown promptly enough and runs 288 times a day.
const SLICE: Duration = Duration::from_secs(5 * 60);

/// Whether the background check is on.
///
/// Absent means on: the design's default is a check every 24 hours, and a setting a user has never seen should not be the
/// reason nothing is ever checked. A value that cannot be read also means on, because the failure mode of "off" is
/// silence about updates and the failure mode of "on" is one request a day.
#[must_use]
pub fn enabled(state: &AppState) -> bool {
    let Ok(rows) = state.db().config() else {
        return true;
    };
    rows.iter()
        .find(|row| row.key == AUTO_UPDATE_KEY)
        .and_then(|row| serde_json::from_str::<bool>(&row.value).ok())
        .unwrap_or(true)
}

/// Asks the registry about every installed plugin.
///
/// # Why per-plugin errors are swallowed
///
/// Because this is a courtesy, not a command: no registry configured, a registry that is down, and a plugin the registry
/// no longer publishes all look the same from here — nothing to report for that plugin — and making the interface explain
/// three network facts on a page the user did not open would be worse than silence. The manual check on the plugins page
/// is where a refusal is named.
#[must_use]
pub fn check_all(state: &AppState) -> Vec<(String, UpdateInfo)> {
    let Ok(rows) = state.db().plugins() else {
        return Vec::new();
    };
    let mut updates = Vec::new();
    for row in rows {
        if let Ok(Some(info)) = crate::commands::registry::check_update(state, &row.plugin_id) {
            updates.push((row.plugin_id, info));
        }
    }
    updates
}

/// Tells the interface how many updates the pass found.
pub fn announce(state: &AppState, updates: &[(String, UpdateInfo)]) {
    if updates.is_empty() {
        return;
    }
    state.publish(KernelEvent::Notice {
        message: "notice.updates".to_owned(),
        params: serde_json::json!({
            "count": updates.len(),
            "plugins": updates.iter().map(|(plugin_id, _)| plugin_id.as_str()).collect::<Vec<_>>(),
        }),
    });
}

/// The loop: check once at start-up, then once a day, until cancelled.
pub fn run(state: &Arc<AppState>, cancel: &Cancellation) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        if enabled(state) {
            let updates = check_all(state);
            if !updates.is_empty() {
                eprintln!(
                    "synra: {} plugin update(s) available: {}",
                    updates.len(),
                    updates.iter().map(|(plugin_id, _)| plugin_id.as_str()).collect::<Vec<_>>().join(", ")
                );
                announce(state, &updates);
            }
        }
        if sleep_unless_cancelled(cancel, INTERVAL) {
            return;
        }
    }
}

/// Sleeps in slices; returns whether the wait was cut short by a cancellation.
fn sleep_unless_cancelled(cancel: &Cancellation, wait: Duration) -> bool {
    let mut left = wait;
    while !left.is_zero() {
        if cancel.is_cancelled() {
            return true;
        }
        let slice = left.min(SLICE);
        std::thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
    cancel.is_cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(plugin: &str) -> (String, UpdateInfo) {
        (
            plugin.to_owned(),
            UpdateInfo {
                version: "2.0.0".to_owned(),
                needs_new_consent: false,
                added_capabilities: Vec::new(),
            },
        )
    }

    #[test]
    fn a_device_that_never_touched_the_setting_checks_for_updates() {
        let state = crate::testing::state();
        assert!(enabled(&state), "absent means on, which is the design's 24-hour default");

        state
            .db()
            .set_config(AUTO_UPDATE_KEY, &serde_json::Value::Bool(false), synra_store::sqlite::now_ms())
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
}
