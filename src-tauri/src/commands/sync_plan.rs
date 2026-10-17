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
use synra_ipc::IpcError;
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

/// What would have to be sent, for one plugin, to every paired device.
///
/// # Where each fact comes from
///
/// `paired_devices` for who could receive it, `active_plugin_version` for what this device has, and `sync_records` for what each
/// peer was last known to hold. Nothing is dialled: a plan is a statement about data, and asking every peer would make the panel
/// wait for the slowest one.
///
/// # What `online` means here, and what it does not
///
/// It means "this device has an address to dial", which is what the sending path needs. It is **not** a liveness check — this
/// shell does not keep a link open — and the interface draws a row it cannot reach differently from one it can.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when the plugin has no active version here: there is nothing to plan a send of, and a plan
/// with no source would be a list of devices to bring up to date with something that does not exist.
pub fn plugins_sync_plan(
    state: &crate::state::AppState,
    plugin_id: &str,
) -> Result<SyncPlanDto, IpcError> {
    use synra_protocol::ErrorCode;

    let id = synra_protocol::ids::PluginId::parse(plugin_id).map_err(|error| {
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

    let records = state.db().sync_records().map_err(read)?;
    let peers: Vec<PeerView> = state
        .db()
        .paired_devices()
        .map_err(read)?
        .into_iter()
        .map(|device| {
            let remote = records
                .iter()
                .find(|record| record.plugin_id == plugin_id && record.peer_device == device.device_id)
                .map(|record| RemoteState {
                    plugin_id: id.clone(),
                    version: Some(record.version.clone()),
                    digest: Some(record.digest.clone()),
                    active: false,
                });
            PeerView {
                device_id: device.device_id.parse().ok().unwrap_or_else(DeviceId::new),
                display_name: device.display_name,
                online: device.address.is_some(),
                remote,
            }
        })
        .collect();

    Ok(plan_for(id, &version, &digest, &peers, ConflictPolicy::NeverDowngrade))
}

#[cfg(test)]
mod tests;
