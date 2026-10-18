//! Acting on a sync plan: sending the artifact each device is missing, and reporting what happened.
//!
//! # What a "run" is
//!
//! The plan said which devices do not have this device's plugin. This command does the sending: one dial per device, a
//! `SyncRound` named by this side, and the receiver's answer read off the same link. The answer is the part that matters —
//! a transfer that was never confirmed says nothing about what the other device holds, and recording it as a success is
//! how the next plan learns to skip a send that never landed.
//!
//! # Why the receiver's answer decides what is recorded
//!
//! Because "the bytes arrived" and "the other device has it" are two facts. The protocol has the receiver confirm the
//! digest (`Verified`) and then say what it did with the package (`SyncResult`); [`Verdict`] is where the three possible
//! answers are separated. The rules are the same ones `SyncTable::apply_result` uses: a refusal records nothing, an
//! "installed" with no version records nothing, and **no answer at all** falls back to the version this device pushed —
//! the best fact available when the peer's build does not send results.
//!
//! # What this does not do, and says so
//!
//! It does not unpack or verify a package on the receiving side, and it does not install one here. The artifact the plan
//! names is a single file; the package format (a manifest and a signature over a directory) does not exist yet. What the
//! receiving host records is therefore "the bytes arrived and this plugin is enabled": see
//! [`record_received`](synra_core::plugin_sync::record_received).

use std::time::Instant;

use synra_core::plugin_sync::SyncRound;
use synra_core::session::SessionEvent;
use synra_ipc::IpcError;
use synra_ipc::dto::{SyncFailure, SyncPlanDto, SyncReport};
use synra_protocol::ErrorCode;
use synra_protocol::ids::{DeviceId, PluginId, SessionId, TraceId};

/// Sends the artifact to every device the plan marks as needing it.
///
/// # The order is the plan's, and it is sequential
///
/// Because each target is one dial and one transfer, and a device that is off its network holds the next one's turn only
/// for the dial's bounded handshake. Running them in parallel would need a worker and a progress channel; a person
/// pressing "sync now" for two or three devices does not need either yet.
///
/// # What `updated` means
///
/// The receiver said it installed the version. Not "the bytes were sent": a transfer that was not confirmed is counted as
/// a failure with the reason, because a plan built on an unconfirmed transfer would skip a send that never happened.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when there is no active version or its artifact is missing on disk, and
/// [`ErrorCode::Unsupported`] for a plugin with no headless half. A per-device failure is **not** an error: it is one
/// entry in the report, because one unreachable device must not stop the others.
pub fn plugins_sync_run(state: &crate::state::AppState, plan: &SyncPlanDto) -> Result<SyncReport, IpcError> {
    let read = |error: synra_store::DbError| {
        IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({"detail": error.to_string()}))
    };

    let Some(active) = state.db().active_plugin(plan.plugin_id.as_str()).map_err(read)? else {
        return Err(IpcError::unknown(ErrorCode::PluginNotInstalled).with_params(serde_json::json!({
            "pluginId": plan.plugin_id.as_str(),
            "reason": "it has no active version here",
        })));
    };

    // ★ An empty artifact name is a UI-only plugin. That is not an error the store should have hidden: there is no
    // headless artifact to send, and a command that opened the plugin's directory with an empty name would try to send
    // the directory itself.
    if active.artifact.is_empty() {
        return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "pluginId": plan.plugin_id.as_str(),
            "reason": "it has no headless artifact; only the UI half exists on this device",
        })));
    }

    let path = state.paths.plugin_dir(plan.plugin_id.as_str()).join(&active.artifact);
    if !path.is_file() {
        return Err(IpcError::unknown(ErrorCode::PluginNotInstalled).with_params(serde_json::json!({
            "pluginId": plan.plugin_id.as_str(),
            "path": path.display().to_string(),
            "reason": "the artifact its active version names is not on disk",
        })));
    }

    // ★ The package, not the bare artifact: the receiver unpacks a `.synra` and runs the installer over it, so what it
    // gets has the manifest, the signature and the UI zip beside the Wasm. It is built once for this round rather than
    // once per device: the bytes are the same, and packaging is a read of the whole plugin directory.
    let archive_dir = state.paths.cache_dir().join("plugin-sync");
    std::fs::create_dir_all(&archive_dir).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": format!("{}: {error}", archive_dir.display())}))
    })?;
    let archive = archive_dir.join(format!(
        "{}-{}.synra",
        plan.plugin_id.as_str().replace(['/', '\\'], "_"),
        active.version
    ));
    synra_core::install::package_directory(&state.paths.plugin_dir(plan.plugin_id.as_str()), &archive)
        .map_err(|refusal| {
            IpcError::unknown(refusal.code()).with_params(serde_json::json!({
                "stage": refusal.stage,
                "reason": refusal.reason,
            }))
        })?;

    let mut report = SyncReport { updated: 0, unchanged: 0, failures: Vec::new() };
    for target in &plan.targets {
        if !target.needs_artifact {
            report.unchanged += 1;
            continue;
        }
        match send_to(state, target.device_id, &plan.plugin_id, &active, &archive) {
            Ok(()) => report.updated += 1,
            Err(error) => report.failures.push(SyncFailure { device_id: target.device_id, error }),
        }
    }
    // The package was this call's to make and to remove; a leftover would be a copy of a plugin in the cache directory
    // that nothing will ever delete.
    let _ = std::fs::remove_file(&archive);
    Ok(report)
}

/// Dials one device, sends the artifact, and records what its answer says it now holds.
fn send_to(
    state: &crate::state::AppState,
    to: DeviceId,
    plugin: &PluginId,
    active: &synra_store::ActiveVersion,
    path: &std::path::Path,
) -> Result<(), IpcError> {
    let address = crate::commands::transfers::address_of(state, to)?;
    let identity = state.identity.lock().clone();
    let now_ms = crate::commands::transfers::unix_millis();
    let failed = |reason: String| {
        IpcError::unknown(ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"deviceId": to.to_string(), "reason": reason}))
    };

    let dialled = synra_core::host::dial_peer(
        &crate::commands::transfers::local_peer(&identity),
        synra_core::peer::Flags::production(),
        address,
        None,
        now_ms,
    )
    .map_err(|error| failed(format!("the dial failed: {error}")))?;
    let mut link = dialled.into_link(identity.device_id, synra_core::peer::Flags::production());

    // ★ The session is named here, by the side that starts the round: two devices pushing at the same moment would each
    // invent one the other has never heard of, and no result could be matched to a round.
    let round = SyncRound {
        plugin_id: plugin.clone(),
        version: active.version.clone(),
        session: SessionId::new(),
    };
    let deadline = Instant::now() + synra_protocol::session::SessionTimeouts::default().transfer_total;

    let events = synra_core::send_bundle(
        &mut link,
        path,
        &round,
        TraceId::new(),
        synra_core::host::millis_reading(now_ms),
        deadline,
    )
    .map_err(|error| failed(error.to_string()))?;

    match verdict(&events, &active.version) {
        Verdict::Installed(version) => {
            state
                .db()
                .record_sync(
                    plugin.as_str(),
                    &to.to_string(),
                    &version,
                    &active.digest,
                    synra_store::sqlite::now_ms(),
                )
                .map_err(|error| {
                    IpcError::unknown(ErrorCode::Internal)
                        .with_params(serde_json::json!({"detail": error.to_string()}))
                })?;
            Ok(())
        }
        Verdict::Refused(why) => Err(failed(why)),
        Verdict::Unconfirmed => Err(failed("the receiver did not confirm the transfer".to_owned())),
    }
}

/// What one finished send means for "does that device hold this plugin now".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It holds this version — the receiver's own answer, or the version this side pushed when the receiver's build
    /// sends no result.
    Installed(String),
    /// It answered, and the package was not installed. Nothing is recorded.
    Refused(String),
    /// The transfer was never confirmed. Nothing is recorded: an unconfirmed transfer says nothing about the peer.
    Unconfirmed,
}

/// Reads a finished send into one of the three answers.
///
/// # The order, and why each rule exists
///
/// `Verified` first: only the receiver's confirmation makes the bytes meaningful, and a result without it would be an
/// answer to a transfer that never completed. Then the result, if there is one: `ok: false` is a refusal (nothing is
/// recorded), and `ok` without a version is a reply that contradicts itself — recording the version this side pushed there
/// would turn "it told me it installed something unnamed" into a fact. No result at all is the honest fallback, because an
/// older build may not send one and the transfer *was* verified.
#[must_use]
pub fn verdict(events: &[SessionEvent], pushed: &str) -> Verdict {
    if !events.iter().any(|event| matches!(event, SessionEvent::Verified { .. })) {
        return Verdict::Unconfirmed;
    }

    let answered = events.iter().find_map(|event| match event {
        SessionEvent::SyncResult { result } => Some(result),
        _ => None,
    });

    match answered {
        Some(result) if !result.ok => Verdict::Refused(
            result.error.clone().unwrap_or_else(|| "it did not install the bundle".to_owned()),
        ),
        Some(result) => match result.installed_version.clone() {
            Some(version) => Verdict::Installed(version),
            None => Verdict::Refused(
                "the receiver said it installed the bundle and named no version".to_owned(),
            ),
        },
        None => Verdict::Installed(pushed.to_owned()),
    }
}

#[cfg(test)]
mod tests;
