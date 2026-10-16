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
    /// When the shell started, for a diagnostic report.
    pub started_at_ms: i64,

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
        Ok(Self {
            host: shared(Host::new(identity.clone(), peer)),
            db: Arc::new(Mutex::new(db)),
            paths,
            identity: Arc::new(Mutex::new(identity)),
            board: Arc::new(Mutex::new(Board::new())),
            plugins: None,
            scanning: Arc::new(Mutex::new(None)),
            scans_started: AtomicU64::new(0),
            bundles_written: AtomicU64::new(0),
            started_at_ms: now_ms,
            metrics: Arc::new(synra_observe::metrics::RingRecorder::new()),
            log: None,
        })
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
