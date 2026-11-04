//! The app adapter for shared leased pointer sessions: pairing, plugin grants, UI state and peer dialing.
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use synra_core::{
    SharedHost,
    input::{Controller, Policy, Receiver},
};
use synra_ipc::{InputPhase, InputSession, InputSnapshot};
use synra_protocol::{DeviceId, PluginId};
use synra_store::SynraDb;

/// One controller and receiver per app, shared by Wasm, host commands and incoming links.
pub struct Input {
    /// Headless plugins feed this controller through the gated WIT call.
    pub controller: Arc<Controller>,
    /// The host UI owns decisions on this receiving desk.
    pub receiver: Arc<Receiver>,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input")
            .field("receiver", &self.receiver.snapshot())
            .finish_non_exhaustive()
    }
}

impl Input {
    /// Build from the same database and host as every other command; do not create an Arc cycle through Host.
    pub fn new(
        db: Arc<Mutex<SynraDb>>,
        host: &SharedHost,
        events: Arc<synra_core::bus::EventBus<synra_ipc::KernelEvent>>,
    ) -> Self {
        Self::with_pointer(db, host, events, Arc::new(synra_platform::pointer::native))
    }

    fn with_pointer(
        db: Arc<Mutex<SynraDb>>,
        host: &SharedHost,
        events: Arc<synra_core::bus::EventBus<synra_ipc::KernelEvent>>,
        factory: Arc<synra_platform::pointer::Factory>,
    ) -> Self {
        let audit_db = Arc::clone(&db);
        let policy: Arc<dyn Policy> = Arc::new(LocalPolicy {
            db,
            host: Arc::downgrade(host),
        });
        let controller = Arc::new(Controller::new(Arc::clone(&policy)));
        let receiver = Arc::new(Receiver::new(
            policy,
            factory,
            Arc::new(move |snapshot| {
                if let Some(state) = &snapshot {
                    let outcome = match (state.phase, state.reason.as_deref()) {
                        (synra_core::input::Phase::Allowed, _) => Some("allowed"),
                        (synra_core::input::Phase::Ended, Some("declined")) => Some("denied"),
                        _ => None,
                    };
                    if let Some(outcome) = outcome {
                        let record = synra_store::records::AuditRecord {
                            plugin_id: state.plugin_id.to_string(),
                            capability: "platform:control-pointer".to_owned(),
                            outcome: outcome.to_owned(),
                            at_ms: synra_store::sqlite::now_ms(),
                            elapsed_ms: None,
                            trace_id: None,
                        };
                        if let Err(error) = audit_db.lock().record_audit(&record) {
                            eprintln!("synra: input consent audit failed: {error}");
                        }
                    }
                }
                events.emit(synra_ipc::KernelEvent::InputUpdated(Box::new(snapshot_of(
                    snapshot,
                ))));
            }),
        ));
        host.lock().set_input_receiver(Arc::clone(&receiver));
        Self {
            controller,
            receiver,
        }
    }

    /// Current wire state, built by the same mapper as lifecycle events.
    pub fn snapshot(&self) -> InputSnapshot {
        snapshot_of(self.receiver.snapshot())
    }

    /// Stop outgoing control and release the receiving pointer before the process exits.
    #[must_use]
    pub fn shutdown(&self) -> bool {
        self.controller.shutdown();
        self.receiver.shutdown()
    }
}

fn snapshot_of(snapshot: Option<synra_core::input::Snapshot>) -> InputSnapshot {
    InputSnapshot {
        session: snapshot.map(|s| InputSession {
            session_id: s.session,
            device_id: s.peer,
            display_name: s.peer_name,
            plugin_id: s.plugin_id,
            phase: match s.phase {
                synra_core::input::Phase::Pending => InputPhase::Pending,
                synra_core::input::Phase::Allowed => InputPhase::Starting,
                synra_core::input::Phase::Active => InputPhase::Active,
                synra_core::input::Phase::Ended => InputPhase::Ended,
            },
            reason: s.reason,
        }),
    }
}

struct LocalPolicy {
    db: Arc<Mutex<SynraDb>>,
    host: Weak<Mutex<synra_core::Host>>,
}

impl Policy for LocalPolicy {
    fn authorize(&self, peer: DeviceId, plugin: &PluginId, receiving: bool) -> Result<i64, String> {
        if receiving && !synra_platform::pointer::supported() {
            return Err("this platform cannot receive pointer input".to_owned());
        }
        let db = self.db.lock();
        let paired = db.paired_devices().map_err(|error| error.to_string())?;
        if !paired
            .iter()
            .any(|row| row.device_id == peer.to_string() && !row.blocked)
        {
            return Err("the controller and computer must be paired and unblocked".to_owned());
        }
        let snapshot = db
            .plugin_ui_snapshot(plugin.as_str())
            .map_err(|error| error.to_string())?
            .filter(|snapshot| snapshot.authority.permits_ui())
            .ok_or_else(|| "the plugin is not enabled and trusted".to_owned())?;
        let grants = synra_capability::CapabilitySet::parse_all(
            snapshot
                .grants
                .iter()
                .filter(|row| row.granted)
                .map(|row| row.capability.as_str()),
        )
        .map_err(|error| error.to_string())?;
        let required = if receiving {
            synra_platform::pointer::CAPABILITY.to_owned()
        } else {
            "device:input".to_owned()
        };
        let capability =
            synra_capability::Capability::parse(&required).map_err(|error| error.to_string())?;
        if !grants.grants(&capability) {
            return Err(format!("missing permission {required}"));
        }
        if !receiving {
            let scoped = synra_capability::Capability::parse(&format!("device:send:{peer}"))
                .map_err(|error| error.to_string())?;
            if !grants.grants(&scoped) {
                return Err("this target is outside the plugin's device grant".to_owned());
            }
        }
        Ok(snapshot.authority.revision)
    }

    fn connect(&self, peer: DeviceId) -> Result<synra_core::PeerLink, String> {
        let host = self
            .host
            .upgrade()
            .ok_or_else(|| "the host has stopped".to_owned())?;
        let address = crate::commands::peers::resolve(&self.db, &host, peer)
            .map_err(|error| format!("{error:?}"))?;
        let (dial, flags) = host.lock().dialling_parts();
        synra_core::dial_peer(
            &dial,
            flags,
            address,
            peer,
            synra_core::reading_millis(synra_store::sqlite::now_ms()),
        )
        .map(synra_core::Dialled::into_link)
        .map_err(|error| error.to_string())
    }

    fn peer_name(&self, peer: DeviceId) -> String {
        self.db
            .lock()
            .paired_devices()
            .ok()
            .and_then(|rows| {
                rows.into_iter()
                    .find(|row| row.device_id == peer.to_string())
            })
            .map_or_else(|| peer.to_string(), |row| row.display_name)
    }
}

#[cfg(test)]
mod tests;
