use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;

use synra_ipc::{IdentitySnapshot, IpcError};
use synra_store::SynraDb;

use super::{AppState, ProcessSample};

impl AppState {
    /// How long this shell has been running.
    #[must_use]
    pub fn uptime(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// Adds the seconds since the last sample to `uptime_seconds`.
    ///
    /// ★ A delta rather than a set. `uptime_seconds` is a counter by design, and a read that overwrote it with the whole
    /// uptime would grow with the number of panel refreshes instead of with time. The first call adds the whole elapsed
    /// time, which is what a process that ran before anyone asked should report.
    pub fn record_uptime(&self, seconds: u64) {
        // The lock is dropped before the counter is touched: `counter_add` takes its own lock, and two locks held in one
        // order here would be a second order somewhere else waiting to deadlock.
        let delta = {
            let mut last = self.uptime_reported.lock();
            let delta = seconds.saturating_sub(*last);
            *last = seconds;
            delta
        };
        self.metrics.counter_add(
            &synra_observe::metrics::SeriesKey::new("uptime_seconds", []),
            delta,
        );
    }

    /// Samples this process for the process gauges.
    ///
    /// Returns `None` when the platform will not describe the process — `sysinfo` has a per-OS `process()` that can
    /// answer nothing, and a metric invented from a missing fact is worse than an absent one.
    #[must_use]
    pub fn sample_process(&self) -> Option<ProcessSample> {
        // The lock lives only as long as the read: the caller then writes the gauges, which take the recorder's own lock,
        // and holding both in one order here would be a second order to deadlock on.
        let mut system = self.process_system.lock();
        let _ =
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.process_pid]), true);
        let process = system.process(self.process_pid)?;
        let memory_bytes = process.memory();
        let cpu_percent = f64::from(process.cpu_usage());
        let threads = process.tasks().map(std::collections::HashSet::len);
        drop(system);
        Some(ProcessSample {
            memory_bytes,
            cpu_percent,
            threads,
        })
    }

    /// Publishes an event for the interface, and answers with its sequence number.
    ///
    /// The number comes from the bus rather than from a counter here: the bus is what a subscriber's gap detection
    /// compares against, and two numbering schemes would disagree the first time anything else published.
    pub fn publish(&self, event: synra_ipc::event::KernelEvent) -> u64 {
        self.events.emit(event)
    }

    /// Starts the shell's own log, in the directory `Paths` keeps logs in.
    ///
    /// # Why this is separate from [`AppState::new`]
    ///
    /// Because `tracing` allows one subscriber per **process**, and `AppState::new` is called by tests as well as by the
    /// window. A constructor that installed a global subscriber would make every test in a binary race for it, and the
    /// tests would then depend on which one ran first. The window calls this; a test does not.
    ///
    /// # Errors
    ///
    /// A message naming what stopped it. The caller decides whether to continue: the shell does, because a window that
    /// will not open is a worse outcome than a diagnostic file that is not there.
    pub fn start_logging(&mut self) -> Result<(), String> {
        let config = synra_observe::logging::LogConfig::in_directory(self.paths.log_dir())
            .with_level(synra_observe::logging::LogLevel::Info);
        let guard = synra_observe::logging::init(&config).map_err(|error| error.to_string())?;
        self.log = Some(guard);
        Ok(())
    }

    /// An identifier for a new scan, unique within this process.
    ///
    /// [`Self::scans_started`] rather than anything persisted, so a restart repeats a number — which is fine, because
    /// the only comparison ever made is against the scan in flight in *this* process.
    #[must_use]
    pub fn next_scan_id(&self) -> String {
        let n = self.scans_started.fetch_add(1, Ordering::Relaxed) + 1;
        format!("scan-{n}")
    }

    /// An identifier for a new diagnostic bundle, unique within this process.
    ///
    /// Separate from [Self::next_scan_id] rather than shared with it, because a bundle and a scan have no reason to
    /// be numbered together and sharing one counter would make the numbering depend on how often the user happened to
    /// scan.
    #[must_use]
    pub fn next_bundle_id(&self) -> u64 {
        self.bundles_written.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The database, locked for as long as the caller needs it.
    ///
    /// A method rather than a public field so that every call site has to say it is holding the lock. A field would let a
    /// command lock it in one place and use it in another, and the guard's scope is exactly what decides whether another
    /// command blocks behind a Wasm call.
    pub fn db(&self) -> parking_lot::MutexGuard<'_, SynraDb> {
        self.db.lock()
    }

    /// Adds the plugin host.
    ///
    /// A builder step rather than a constructor argument so that a shell which does not run plugins is constructible at
    /// all. The two are different configurations, not one with a default.
    #[must_use]
    pub fn with_plugins(mut self, plugins: Arc<Mutex<synra_core::dispatch::PluginSet>>) -> Self {
        self.plugins = Some(plugins);
        self
    }

    /// Records what the startup load pass did.
    ///
    /// A builder step rather than a constructor argument, because it is only known after the engine is built — and the
    /// engine is the host's own decision ([`Self::with_plugins`] explains why it may be absent at all).
    #[must_use]
    pub fn with_plugin_loads(mut self, loads: synra_core::plugin::LoadedSummary) -> Self {
        *self.plugin_loads.get_mut() = loads;
        self
    }

    /// Records why no engine was built.
    #[must_use]
    pub fn with_engine_refusal(mut self, reason: impl Into<String>) -> Self {
        self.engine_refusal = Some(reason.into());
        self
    }

    /// The startup load report, for a command that has to explain a card.
    pub fn plugin_loads(&self) -> parking_lot::MutexGuard<'_, synra_core::plugin::LoadedSummary> {
        self.plugin_loads.lock()
    }

    /// Notes that one plugin is now loaded, without re-running the whole pass.
    ///
    /// Used by the commands that change a plugin's life after startup — enabling, updating — so their answer is the
    /// same one the list reads. The lock is taken and released here; a caller must not hold it while loading, because
    /// compiling a plugin under the same lock every card read takes is a list that stops while a plugin compiles.
    pub fn note_plugin_loaded(&self, id: synra_protocol::ids::PluginId) {
        self.plugin_loads.lock().record_loaded(id);
    }

    /// Notes that one plugin failed to load, replacing any earlier claim that it did.
    pub fn note_plugin_refused(&self, plugin_id: &str, reason: String) {
        self.plugin_loads.lock().record_refused(plugin_id, reason);
    }

    /// Adds the discovery worker the scan needs.
    ///
    /// A builder step for the same reason [`AppState::with_plugins`] is one: a shell that could not bind the discovery
    /// sockets is a real configuration rather than a broken one, and every other command works in it.
    #[must_use]
    pub fn with_discovery(mut self, discovery: crate::discovery::Discovery) -> Self {
        self.discovery = Some(discovery);
        self
    }

    /// What this device announces to the network.
    ///
    /// # Why the port is an argument rather than a field
    ///
    /// Because it is the port a peer *connects* to, and this build has none: see [`crate::discovery`]'s header for what the
    /// shell announces instead, and why the wire format leaves it no way to say "nowhere".
    ///
    /// Built from the identity rather than kept as a field, because the identity is what changes: a user who renames this
    /// device expects the next announcement to carry the new name, and a copy taken at start-up would keep announcing the
    /// old one until the process restarted.
    #[must_use]
    pub fn announcement(&self, port: u16) -> synra_protocol::discovery::DeviceAnnouncement {
        let identity = self.identity.lock();
        synra_core::LocalPeer::new(
            identity.device_id,
            identity.display_name.clone(),
            synra_core::platform_of_os(&identity.platform),
            crate::KERNEL_VERSION,
        )
        .announcement(port)
    }

    /// The plugin set, or a refusal that says why there is not one.
    ///
    /// # Errors
    ///
    /// [`IpcError::Unsupported`] when this build has no engine. The message carries the reason, because the person
    /// reading it is looking at a plugin that will not work and needs to know whether to wait or give up.
    pub fn plugin_set(&self) -> Result<&Arc<Mutex<synra_core::dispatch::PluginSet>>, IpcError> {
        self.plugins.as_ref().ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
                .with_params(serde_json::json!({"reason": "this build has no plugin engine"}))
        })
    }

    /// This device, as the interface is shown it.
    #[must_use]
    pub fn identity_snapshot(&self, kernel_version: &str) -> IdentitySnapshot {
        let identity = self.identity.lock();
        IdentitySnapshot {
            device_id: identity.device_id,
            display_name: identity.display_name.clone(),
            platform: identity.platform.clone(),
            kernel_version: kernel_version.to_owned(),
            // ★ Compared against the kernel's own default rather than stored as a flag.
            //
            // A flag would be a second thing to keep true, and the moment it drifted the interface would prompt a user who
            // had already chosen a name. The trade is that a user who deliberately names their device exactly
            // `DEFAULT_DISPLAY_NAME` is reported as having the default — which costs one unnecessary prompt, and buys the
            // guarantee that a prompt is never shown to someone who did not need one.
            name_is_default: identity.display_name == synra_core::DEFAULT_DISPLAY_NAME,
        }
    }
}
