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
const CHECK_EVERY_MS: i64 = 24 * 60 * 60 * 1000;

/// How long one sleep slice is.
///
/// Not 250 ms: the wait between checks is a day, and waking four times a second for it would be a busy loop that costs
/// more than the check. Five minutes is short enough that turning the switch off takes effect in this cycle rather than
/// tomorrow.
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
pub fn check_all(state: &AppState) -> Vec<UpdateInfo> {
    let Ok(rows) = state.db().plugins() else {
        return Vec::new();
    };
    // ★ Cleared before the pass, not after: `check_update` writes the cache, and an entry left from a plugin that was
    // uninstalled since the last pass would be an update for something this device no longer has.
    state.available_updates.lock().clear();
    let mut updates = Vec::new();
    for row in rows {
        if let Ok(Some(info)) = crate::commands::registry::check_update(state, &row.plugin_id) {
            updates.push(info);
        }
    }
    updates
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

/// Whether a check is due, given when the last one ran.
///
/// `None` means never, and a clock that went backwards reads as due: a check is cheaper than silence about an update.
#[must_use]
fn due(last_ms: Option<i64>, now_ms: i64) -> bool {
    match last_ms {
        None => true,
        Some(last) => now_ms < last || now_ms.saturating_sub(last) >= CHECK_EVERY_MS,
    }
}

/// The loop: check once at start-up, then once a day, until cancelled.
///
/// # Why the wait is walked in five-minute steps
///
/// Because the setting can change while the loop is asleep, and a single sleep for a day would make "off" mean "off from
/// tomorrow". The steps do not make the check more frequent: [`due`] still allows one every 24 hours.
pub fn run(state: &Arc<AppState>, cancel: &Cancellation) {
    let mut last_ms: Option<i64> = None;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let now = synra_store::sqlite::now_ms();
        if enabled(state) && due(last_ms, now) {
            let updates = check_all(state);
            last_ms = Some(now);
            if !updates.is_empty() {
                eprintln!(
                    "synra: {} plugin update(s) available: {}",
                    updates.len(),
                    updates.iter().map(|info| info.plugin_id.as_str()).collect::<Vec<_>>().join(", ")
                );
                announce(state, &updates);
            }
        }
        if sleep_unless_cancelled(cancel, SLICE) {
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

    #[test]
    fn a_check_is_due_once_a_day_and_after_never() {
        // ★ The bound is on the check, not on the sleep: the loop wakes every five minutes so a setting change takes
        // effect, and this is what keeps those wakes from becoming registry requests.
        assert!(due(None, 0), "a device that has never checked is due");
        assert!(!due(Some(1_000), 1_000 + CHECK_EVERY_MS - 1), "not yet");
        assert!(due(Some(1_000), 1_000 + CHECK_EVERY_MS), "the day has passed");
        assert!(due(Some(10_000), 1_000), "a clock that went backwards is due, not silent");
    }
}
