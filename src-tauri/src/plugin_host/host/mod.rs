use super::*;
use crate::{
    basic_host_methods, network_host_methods, platform_host_methods, storage_actions_host_methods,
};

mod basic;
mod network;
mod platform;
mod storage_actions;

impl Host for DesktopHost {
    basic_host_methods!();
    platform_host_methods!();
    network_host_methods!();
    storage_actions_host_methods!();
}

/// Where a plugin's audit records go, besides the ring.
///
/// # ★ Why the shell needs one, and did not have one
///
/// `PluginSet::with_audit_sink` was written for the daemon, and the daemon was the only host that called it. So in the
/// desktop shell a plugin's audit trail lived in the in-memory ring and **died with the process** — which is the exact gap
/// the sink exists to close, and the reason the `plugin_audit` migration says what the table is for: *"the durable part is
/// what makes the interface's 'recent activity' survive a restart."* A user who quit the app and opened it again saw an
/// empty history, and nothing anywhere said the history had never been written down.
///
/// # Why the write is logged and not refused
///
/// Because the call it describes has already happened. Refusing afterwards would tell a plugin its action did not occur when
/// it did, and an audit trail is a record of what happened rather than a gate on it — the same reasoning the daemon's sink
/// follows.
#[must_use]
pub fn audit_sink(db: Arc<Mutex<SynraDb>>) -> synra_core::dispatch::plugins::AuditSink {
    Arc::new(move |entry: &synra_observe::audit::AuditEntry| {
        let record = synra_store::records::AuditRecord {
            plugin_id: entry.plugin.as_str().to_owned(),
            capability: entry.capability.to_string(),
            outcome: entry.outcome.as_str().to_owned(),
            at_ms: entry.at_ms,
            elapsed_ms: entry
                .elapsed_ms
                .map(|elapsed| i64::try_from(elapsed).unwrap_or(i64::MAX)),
            // ★ The action's trace, when the call had one. `can-handle` and lifecycle calls do not, and `None` says that
            // rather than making every row look like part of a user action.
            trace_id: entry.trace.map(|trace| trace.to_string()),
        };
        if let Err(error) = db.lock().record_audit(&record) {
            // The app has no `tracing`, so this is what the shell reports through — the same channel `main.rs` uses for the
            // plugin host's own failures.
            eprintln!(
                "synra: an audit record for {} could not be written down: {error}. The call it describes has already happened.",
                entry.plugin.as_str()
            );
        }
    })
}
