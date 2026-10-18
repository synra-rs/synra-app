//! Turning "we have this, they have that" into the plan the interface shows.
//!
//! # Why this lives in the shell
//!
//! Because it is the only layer that can see both sides: the kernel's [`decide`] gives a judgement about one device, `online` is a fact
//! about this shell's links, and [`SyncPlanDto`] is a wire type. The kernel deliberately does not depend on the wire, so the assembly
//! belongs where both are visible.
//!
//! # Why the peers are asked rather than read from a table
//!
//! ★ **2026-10-18: this command used to read `sync_records` and dial nothing, and that was wrong in a way the code could not see.**
//! The table is only written when a bundle moves — so on a new pairing it says every peer holds nothing, and the plan tells the user to
//! send every plugin. That is safe, and it is also wrong: it wastes a transfer and it cannot ever notice a plugin the peer already has.
//!
//! A plan is a statement about **now**, so it asks each paired device what it holds and records the answer. The exchange is a
//! request/response of its own ([`ask_plugin_list`]), not a transfer: it moves one small envelope, and the receiver answers from its own
//! tables. Recording the answer is what keeps `sync_records` fresh for every other reader — the plan is the natural moment to ask,
//! because it is the moment somebody wants to know.
//!
//! # What it costs, and what it still cannot know
//!
//! One dial per paired device, bounded by [`HELLO_TIMEOUT`]. A device that does not answer is reported with `online: false` and counted as
//! needing the artifact — "we could not ask" is not "it has it", and dropping the row would make the panel claim a sync that never
//! happened. `online` therefore means **it answered**, not "it has an address": the interface draws an unreachable row differently from a
//! reachable one.

use std::net::SocketAddr;
use std::time::Instant;

use synra_core::plugin_sync::{ConflictPolicy, RemoteState, SyncAction, ask_plugin_list, decide};
use synra_ipc::dto::{SyncPlanDto, SyncTarget};
use synra_ipc::IpcError;
use synra_protocol::ids::{DeviceId, PluginId, TraceId};
use synra_protocol::limits::HELLO_TIMEOUT;
use synra_protocol::session::PluginList;

/// What one device holds of the plugin being planned.
pub struct PeerView {
    /// The device.
    pub device_id: DeviceId,
    /// What it calls itself.
    pub display_name: String,
    /// Whether it **answered** the question. Not "it has an address": a device with an address that did not answer is unreachable,
    /// and the interface must be able to tell the two apart.
    pub online: bool,
    /// Its answer to "what do you have", or `None` when it did not answer.
    pub remote: Option<RemoteState>,
}

/// The plan for one plugin, across the devices asked.
///
/// # Why an unanswered device is reported rather than skipped
///
/// Because "we could not ask" and "it does not have it" lead to different actions: the first is a device to try again, the second is
/// one to send to. Dropping the unanswered ones would make the panel say the device already had the plugin, which is the failure a
/// sync exists to avoid.
#[must_use]
pub fn plan_for(
    plugin_id: PluginId,
    our_version: &str,
    our_digest: &str,
    peers: &[PeerView],
    policy: ConflictPolicy,
) -> SyncPlanDto {
    let targets = peers
        .iter()
        .map(|peer| {
            // An unanswered device is treated as one with nothing: that makes `decide` push, and `online` is what tells the
            // interface it has to wait for a link first.
            let theirs = peer.remote.clone().unwrap_or_else(|| RemoteState::absent(plugin_id.clone()));
            let needs_artifact = matches!(
                decide(our_version, our_digest, &theirs, policy),
                SyncAction::Push { .. }
            );
            SyncTarget {
                device_id: peer.device_id,
                display_name: peer.display_name.clone(),
                needs_artifact,
                version: theirs.version.clone(),
                online: peer.online,
            }
        })
        .collect();

    SyncPlanDto { plugin_id, targets }
}

/// Reads one peer's answer into this plugin's state and the rows that describe what it holds.
///
/// # The two answers, and why both are needed
///
/// The state for **this** plugin is what the plan compares. The rows are the whole list, because `sync_records` answers the same question
/// for every plugin and a plan about one of them is no reason to throw the rest away. A summary with neither a version nor a digest
/// claims nothing and is not recorded — an empty row would read as "it holds the empty artifact".
#[must_use]
pub fn interpret(plugin_id: &PluginId, answer: &PluginList) -> (RemoteState, Vec<(String, String, String)>) {
    let held = answer
        .plugins
        .iter()
        .filter(|summary| summary.version.is_some() || summary.digest.is_some())
        .map(|summary| {
            (
                summary.plugin_id.as_str().to_owned(),
                summary.version.clone().unwrap_or_default(),
                summary.digest.clone().unwrap_or_default(),
            )
        })
        .collect();

    let remote = answer
        .plugins
        .iter()
        // ★ A summary that claims neither a version nor a digest is read as "it does not have it", not as a state with no version:
        // `RemoteState::has_it` already refuses to call that holding it, and a state whose `active` flag says otherwise would be the
        // only place in this exchange where "has it" had two answers.
        .find(|summary| summary.plugin_id == *plugin_id && (summary.version.is_some() || summary.digest.is_some()))
        .map_or_else(|| RemoteState::absent(plugin_id.clone()), RemoteState::from_summary);

    (remote, held)
}

/// Asks one device what plugins it has.
///
/// # Why this is not a silent `Option`
///
/// Because the caller reports the reason to its log, and "it has no address", "the address does not parse" and "it did not answer"
/// are three different failures: the first needs a connection, the second is stored data that needs fixing, and the third needs the
/// device to be switched on.
fn ask(state: &crate::state::AppState, at: &str, to: DeviceId) -> Result<PluginList, String> {
    let address = at.parse::<SocketAddr>().map_err(|error| format!("{at}: {error}"))?;
    let identity = state.identity.lock().clone();
    let now_ms = unix_millis();
    let dialled = synra_core::host::dial_peer(
        &crate::commands::transfers::local_peer(&identity),
        synra_core::peer::Flags::production(),
        address,
        None,
        now_ms,
    )
    .map_err(|error| error.to_string())?;
    let mut link = dialled
        .into_link(identity.device_id, synra_core::peer::Flags::production())
        // Same recorder as every other dial: the byte counters are per link, so a link that skips it is one the panel
        // cannot describe.
        .with_metrics(std::sync::Arc::clone(&state.metrics));
    // ★ The same figure the listener uses for "how long may a peer take to speak". A shorter one here would call a slow device
    // offline while it was still answering; a longer one would make the panel wait for a device that is gone.
    link.set_read_timeout(Some(HELLO_TIMEOUT));
    ask_plugin_list(
        &mut link,
        identity.device_id,
        to,
        TraceId::new(),
        synra_core::host::millis_reading(now_ms),
        Instant::now() + HELLO_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

/// What would have to be sent, for one plugin, to every paired device.
///
/// # Where each fact comes from
///
/// `paired_devices` for who is asked, `active_plugin_version` for what this device has, and the peers themselves for what they hold.
/// Each answer replaces that peer's row in `sync_records`, so the table is fresh for anything else that reads it.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when the plugin has no active version here: there is nothing to plan a send of, and a plan
/// with no source would be a list of devices to bring up to date with something that does not exist.
pub fn plugins_sync_plan(state: &crate::state::AppState, plugin_id: &str) -> Result<SyncPlanDto, IpcError> {
    use synra_protocol::ErrorCode;

    let id = PluginId::parse(plugin_id).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}))
    })?;

    let read = |error: synra_store::DbError| {
        IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({"detail": error.to_string()}))
    };

    let Some((version, digest)) = state.db().active_plugin_version(id.as_str()).map_err(read)? else {
        return Err(IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": "it has no active version here"})));
    };

    let devices = state.db().paired_devices().map_err(read)?;
    let asked_at_ms = synra_store::sqlite::now_ms();
    let peers: Vec<PeerView> = devices
        .into_iter()
        .map(|device| {
            let to = device.device_id.parse::<DeviceId>().ok();
            let answer = match (&device.address, to) {
                (Some(address), Some(to)) => ask(state, address, to)
                    .inspect_err(|why| {
                        eprintln!("synra: {} could not be asked what it holds: {why}", device.display_name);
                    })
                    .ok(),
                (None, _) => {
                    eprintln!("synra: {} has no address this device has dialled", device.display_name);
                    None
                }
                (Some(_), None) => {
                    eprintln!("synra: {} is stored under an identifier this build cannot read", device.display_name);
                    None
                }
            };
            match answer {
                Some(list) => {
                    let (remote, held) = interpret(&id, &list);
                    // ★ The answer is recorded before the plan is built, so a `sync_records` read after this command — including one
                    // that fails halfway through this loop — describes peers that really answered rather than only peers that moved a
                    // bundle once. A failure to record is reported and does not change the plan: the answer is still in hand.
                    if let Err(error) = state.db().replace_sync_for_peer(&device.device_id, &held, asked_at_ms) {
                        eprintln!(
                            "synra: {}'s answer could not be recorded: {error}",
                            device.display_name
                        );
                    }
                    PeerView {
                        device_id: to.unwrap_or_else(DeviceId::new),
                        display_name: device.display_name,
                        online: true,
                        remote: Some(remote),
                    }
                }
                None => PeerView {
                    device_id: to.unwrap_or_else(DeviceId::new),
                    display_name: device.display_name,
                    online: false,
                    remote: None,
                },
            }
        })
        .collect();

    Ok(plan_for(id, &version, &digest, &peers, ConflictPolicy::NeverDowngrade))
}

/// Milliseconds since the epoch, as the wire's `u64`.
fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_u64, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests;
