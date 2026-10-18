//! What a command has to reach to do its work.
//!
//! # Why this is concrete and not a trait
//!
//! Because a trait here would have no production implementor. The kernel already runs with no window — `synra-daemon`
//! is the proof, and it is the reason the kernel's boundary is trustworthy — so the shell does not need an abstraction
//! to make itself testable. It needs the same pieces the daemon holds, and a test builds them over an in-memory
//! database.
//!
//! Writing a thirty-eight-method trait anyway would be the shape that feels safe and is not: it would be implemented
//! once by production and once by every test, and the two would drift until the tests described a kernel that does not
//! exist.
//!
//! # Why the pieces sit behind separate locks
//!
//! Because they block for different reasons, and often enough that sharing one lock would be a defect rather than a
//! simplification. A Wasm call is bounded by fuel and a clock and can still take its whole budget; a socket read waits
//! for a peer; a database call is milliseconds. A `devices_snapshot` that waited behind a compiling plugin would be the
//! kind of bug nobody finds until a user with nine installed plugins says the app is slow.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use synra_core::intent::Board;
use synra_core::{Host, Identity, SharedHost, shared};
use synra_ipc::{IpcError, IdentitySnapshot, ScanState};
use synra_store::{Paths, SynraDb};

/// The kernel pieces the shell's commands reach for.
///
/// # Why `plugins` may be absent
///
/// Because a shell with no engine is a real configuration — the windowless kernel, or a build for a platform where
/// plugins are not embedded. A field of `Option` means every command has to answer "what should a user be told when
/// there is no plugin host?" rather than assuming one exists. [`AppState::plugin_set`] is where that is answered, and
/// it answers with a refusal naming the situation instead of an empty list.
#[derive(Debug)]
pub struct AppState {
    /// The database, which is what survives a restart.
    ///
    /// ★ Behind a lock, and that is not a style choice.
    ///
    /// `rusqlite::Connection` is `Send` but **not** `Sync`, so a bare `Arc<SynraDb>` makes this struct neither — and
    /// Tauri refuses to register a state that is not. `Mutex<SynraDb>` is `Sync` whenever its contents are `Send`, which
    /// is the same reason the kernel stores its one connection behind a single owner. The cost is that two commands
    /// touching the database serialise, and the benefit is that the shell runs at all.
    pub db: Arc<Mutex<SynraDb>>,
    /// Where everything on disk lives.
    pub paths: Paths,
    /// This device.
    pub identity: Arc<Mutex<Identity>>,
    /// The peer registry and the links out of it.
    pub host: SharedHost,
    /// The intents in flight, and what each is waiting for.
    pub board: Arc<Mutex<Board>>,
    /// The plugins this shell can ask, when it has an engine.
    pub plugins: Option<Arc<Mutex<synra_core::dispatch::PluginSet>>>,
    /// The discovery worker, when this shell could bind the sockets.
    ///
    /// # Why this may be absent, and what that costs
    ///
    /// A machine with no network, a firewall and a sandbox all produce the same bind failure, and none of them is a reason
    /// to lose the window — the same decision [`AppState::plugins`] makes for an engine that will not start. What it costs
    /// is the **scan**: `devices_scan_start` refuses a network scan with the reason, and the cached read — which is what a
    /// list of already-known devices needs — still answers. See [`crate::discovery`].
    pub discovery: Option<crate::discovery::Discovery>,
    /// The scan in progress, if any.
    pub scanning: Arc<Mutex<Option<ScanState>>>,
    /// How many scans this process has started.
    ///
    /// A counter rather than a random identifier, and the reason is narrow: a scan identifier is only ever compared
    /// against the one scan in flight, so uniqueness has to hold within a process and nowhere else. A random one would
    /// also be unique, but it would be unreadable in a log line, and a log line is where a user-reported scan shows up.
    pub scans_started: AtomicU64,

    /// Diagnostic bundles written by this process.
    ///
    /// A counter rather than a clock, because a user who exports twice inside one second is the ordinary case —
    /// they are comparing a bundle from before a change with one from after it — and a name built from the second
    /// would silently put the second file on top of the first.
    pub bundles_written: AtomicU64,
    /// The host the listener serves, once it has bound a port.
    ///
    /// Set before with_discovery_or_report, because the announcement has to carry the port peers should **dial** — the
    /// listener's — and a device that announced the discovery port would be one whose pairing always dialled the wrong service.
    pub listener: Option<(Arc<Mutex<synra_core::Host>>, u16)>,
    /// The pairing desk: the requests waiting for a person, and the answers going back.
    ///
    /// ★ Held **here** rather than inside the listener, and that is what makes pairing work at all: a request arrives on a
    /// socket thread, the user answers from a command, and those are different threads with no connection between them. The
    /// desk is the shared thing they agree on — the daemon learnt this first, where the answer comes from a person who is not
    /// on the connection that asked.
    pub pairing: Arc<synra_core::pairing::PairingDesk>,
    /// The transfer desk: incoming transfers waiting for this device's answer.
    ///
    /// ★ Beside the pairing desk and for the same reason: the question is asked on a connection thread and answered by a
    /// command, and those are different threads with no other value between them.
    pub transfer_desk: Arc<crate::transfer_desk::TransferDesk>,
    /// The chunked uploads a plugin page is writing through `transfer.begin` / `chunk` / `finish`.
    pub uploads: crate::commands::plugin_ui::Uploads,
    /// When the shell started, for a diagnostic report.
    pub started_at_ms: i64,

    /// The desktop platform: the clipboard, the browser and notifications.
    ///
    /// ★ One instance, shared with the plugin host. The platform keeps per-instance state — the notification limiter —
    /// so a page calling through a second instance would get a second hourly allowance for the same user.
    pub platform: Arc<synra_platform::CommandPlatform>,

    /// The shell's own metrics, filled by the plugin gate.
    ///
    /// ★ Why the shell holds one, which is the same story as the log ring below.
    ///
    /// `diag_metrics` is one of the kernel's commands and this shell did not implement it, with the reason written in the
    /// registration test: *"no metrics sink is behind `AppState`"*. That is an honest refusal, and it is also a gap — the
    /// **daemon** has had a recorder since the gate learned to count, and a plugin asking for a capability it was not
    /// granted is exactly as interesting in the window as it is on a headless machine.
    ///
    /// The recorder is shared rather than duplicated, and read through `snapshot`, so what the command reports and what the
    /// gate counted cannot drift apart.
    pub metrics: Arc<synra_observe::metrics::RingRecorder>,

    /// The log subscription, when it could be started.
    ///
    /// # ★ Why the shell holds it rather than the daemon
    ///
    /// Because they are two processes with two logs. The daemon writes the kernel's log and answers `synra logs`; the
    /// shell writes its own, and until this field existed the shell had none — so `diag_export` reported the debug-log
    /// section as an **empty list** rather than as an absence, and the comment beside it said so: "there is no log ring
    /// behind `AppState` today".
    ///
    /// It is held rather than leaked because dropping it is what flushes the file: a guard that went out of scope
    /// immediately would close the log on the first line, and one that was leaked would never flush the last.
    ///
    /// `None` when the file could not be opened. The shell still runs — a window that refuses to open because it cannot
    /// write a diagnostic file trades the user's work for its record — and `diag_export` says the section is missing
    /// instead of claiming there was nothing to log.
    pub log: Option<synra_observe::logging::LogGuard>,

    /// What the kernel tells the interface about, as it happens.
    ///
    /// # ★ Why the shell needed one
    ///
    /// Because every page was a snapshot and nothing else: the frontend has had a `SequenceTracker` and an event
    /// dispatcher for two rounds, and no producer — so a plugin enabled in another window, a scan that finished while a
    /// dialog was open, and a consent decision were all invisible until the user navigated away and back.
    ///
    /// The bus is the kernel's own ([`synra_core::bus::EventBus`]), not a second one built here: it numbers events,
    /// bounds each subscriber's queue and says when a subscriber was too slow. The payloads are the **wire** events
    /// ([`synra_ipc::event::KernelEvent`]) rather than the kernel's internal ones, because the interface is the only
    /// consumer of this stream and a mapping layer between two enums would be a second place for the two to drift.
    pub events: Arc<synra_core::bus::EventBus<synra_ipc::event::KernelEvent>>,
    /// The tokens issued to plugin pages, and the validation `plugin_ui_ready` performs against them.
    ///
    /// The registry is the kernel's, which is where issuing and revoking live; this shell only ever asks it a question.
    pub ui_sessions: Arc<synra_plugin_ui::UiSessionRegistry>,
    /// Which plugin page is in which WebView.
    ///
    /// ★ The identity a token cannot carry. `UiSessionRegistry::validate` needs the **claiming** plugin and its running
    /// version, and a token is a nonce: it says "some page holds this", not "this page is plugin X". The window that
    /// created the WebView knows both, so it records them here and `plugin_ui_ready` looks the label up.
    pub hosted_ui: Mutex<std::collections::HashMap<String, HostedUi>>,
    /// The transfers this shell is running.
    ///
    /// Written by the worker that moves the chunks, read by `transfer_list`. A registry rather than a read from the link: a
    /// transfer outlives the frame that started it, so the interface has to be able to ask between two of them.
    pub transfers: crate::commands::transfers::Transfers,
    /// Inbound transfers whose receiver asked to cancel.
    ///
    /// ★ A command cannot send the abort: the listener's connection thread is the only writer to that link, and a second
    /// writer would race it. The command leaves the identifier here, and the loop consumes it between messages.
    pub inbound_cancels: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Static registry documents, by URL, with the validators their hosts sent.
    ///
    /// ★ A conditional request is the whole point: a bucket's `index.json` changes rarely, and a page that re-read it on
    /// every visit pays for a body it already has. The cache is keyed by URL and bounded by the process, not by time —
    /// a document that answered 304 is the same document.
    pub registry_cache: Arc<Mutex<std::collections::HashMap<String, crate::commands::registry::CachedDocument>>>,
}

/// One plugin page this shell opened, as the window that opened it knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedUi {
    /// The plugin the page belongs to.
    pub plugin_id: synra_protocol::ids::PluginId,
    /// The version it was opened for, which validation compares against the session's.
    pub version: String,
    /// The token's nonce, as issued to this page.
    pub nonce: u128,
    /// Whether the page has said it is ready. A page that never does is a page that never drew.
    pub ready: bool,
}

impl AppState {
    /// A state over an already-open database, with no plugin host.
    ///
    /// # Errors
    ///
    /// [`IpcError`] when the identity cannot be read or written. A shell with no identity has nothing to show a user
    /// and nothing to present to a peer, so this is a start-up refusal rather than a degraded mode.
    pub fn new(db: SynraDb, paths: Paths, platform: &str, now_ms: i64) -> Result<Self, IpcError> {
        let identity = Identity::load(&db, platform, now_ms).map_err(internal)?;
        // Written back because `load` generates one on a fresh installation, and a generated identity that is only in
        // memory is a device that appears new to every peer on every start — which loses every pairing, silently.
        identity.store(&db, now_ms).map_err(internal)?;
        let peer = synra_core::LocalPeer::new(
            identity.device_id,
            identity.display_name.clone(),
            synra_core::platform_of_os(platform),
            env!("CARGO_PKG_VERSION"),
        );
        // Read before the identity is moved into its Arc below: the desk needs the identifier and the field takes ownership.
    let local_id = identity.device_id;

    // ★ Built before the host, because the host is given one.
    //
    // `Host::with_metrics` existed for the daemon and the window built its host without one, so `frames_sent_total`,
    // `frames_received_total` and `links_opened_total` were written by nothing in the one host a desktop user runs.
    let metrics = Arc::new(synra_observe::metrics::RingRecorder::new());

    let state = Self {
            host: shared(Host::new(identity.clone(), peer).with_metrics(Arc::clone(&metrics))),
            db: Arc::new(Mutex::new(db)),
            paths,
            identity: Arc::new(Mutex::new(identity)),
            board: Arc::new(Mutex::new(Board::new())),
            plugins: None,
            discovery: None,
            scanning: Arc::new(Mutex::new(None)),
            scans_started: AtomicU64::new(0),
            bundles_written: AtomicU64::new(0),
            listener: None,
            pairing: Arc::new(synra_core::pairing::PairingDesk::new(
                local_id,
                synra_protocol::session::SessionTimeouts::default().pairing,
            )),
            transfer_desk: Arc::new(crate::transfer_desk::TransferDesk::new()),
            uploads: crate::commands::plugin_ui::Uploads::new(),
            started_at_ms: now_ms,
            platform: Arc::new(synra_platform::CommandPlatform::new()),
            metrics,
            log: None,
            events: Arc::new(synra_core::bus::EventBus::new()),
            ui_sessions: Arc::new(synra_plugin_ui::UiSessionRegistry::new()),
            hosted_ui: Mutex::new(std::collections::HashMap::new()),
            transfers: crate::commands::transfers::Transfers::new(),
            inbound_cancels: Arc::new(Mutex::new(std::collections::HashSet::new())),
            registry_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

    // ★ One prune per construction, not per request: a cache whose files are never removed is a directory that grows with
    // every registry this device ever asked about. A failure to prune is a diagnostic, not a reason to refuse to start.
    crate::commands::registry::prune_documents(&state, now_ms);

    // ★ And the audit table gets the same treatment for the same reason: the sink writes a row on every gated call, and
    // nothing in this shell swept them. The retention rule is the design's seven days; see `prune_audit`.
    crate::commands::plugins::prune_audit(&state, now_ms);
    Ok(state)
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

/// Turns a storage error into the refusal a command returns.
///
/// Named rather than written inline because it appears in every command that reads the database, and the one thing all
/// of them must agree on is that a storage failure is `internal` and not `invalid_argument`: a user who sent a valid
/// request does not need to be told they sent it wrong.
pub(crate) fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(synra_protocol::ErrorCode::Internal)
        .with_params(serde_json::json!({"detail": error.to_string()}))
}

#[cfg(test)]
mod tests {
    use synra_observe::metrics::Value;

    #[test]
    fn the_shells_host_counts_into_the_shells_recorder() {
        // ★ The host had no recorder before this: `Host::with_metrics` existed for the daemon, and the window built its
        // host without one — so the frame and link counters were zero in the only host a desktop user runs. The counter
        // is public precisely so a host can be shown to have one; the assertion is on the recorder the rest of the shell
        // reads (`diag_metrics`), not on a private field.
        let state = crate::testing::state();

        state.host.lock().count_frame_sent("kernel.test");

        assert_eq!(
            state.metrics.snapshot().get("frames_sent_total{event=kernel.test}"),
            Some(&Value::Counter(1)),
            "the host's frame counter is the shell's recorder"
        );
    }
}
