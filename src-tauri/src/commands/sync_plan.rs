//! Turning "we have this, they have that" into the plan the interface shows.
//!
//! # Why this lives in the shell
//!
//! Because it is the only layer that can see both sides: the kernel's [`decide`] gives a judgement about one device, `needs_artifact`
//! and `online` are facts about this shell's links, and [`SyncPlanDto`] is a wire type. The kernel deliberately does not depend on
//! the wire, so the assembly belongs where both are visible.
//!
//! # Why it is a pure function
//!
//! Because a plan is a decision about data, not an action: nothing is dialled or written here, so a test can hand it a table and
//! read the answer. The dialling belongs to the command that calls this.

use synra_core::plugin_sync::{ConflictPolicy, RemoteState, SyncAction, decide};
use synra_ipc::dto::{SyncPlanDto, SyncTarget};
use synra_protocol::ids::DeviceId;

/// What one device holds of the plugin being planned.
pub struct PeerView {
    /// The device.
    pub device_id: DeviceId,
    /// What it calls itself.
    pub display_name: String,
    /// Whether this shell has a live link to it.
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
    plugin_id: synra_protocol::ids::PluginId,
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

#[cfg(test)]
mod tests;
