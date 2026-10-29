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
use synra_core::supervisor::Cancellation;
use synra_core::{Host, Identity, SharedHost, shared};
use synra_ipc::{IdentitySnapshot, IpcError, ScanState};
use synra_store::{Paths, SynraDb};

/// One admitted network scan; cancellation is a request until its owner exits.
#[derive(Debug)]
pub(crate) struct ScanTask {
    pub snapshot: ScanState,
    pub cancellation: Cancellation,
}

impl ScanTask {
    pub fn new(snapshot: ScanState) -> Self {
        Self {
            snapshot,
            cancellation: Cancellation::new(),
        }
    }
}

/// One read of this process's facts, for the process gauges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessSample {
    /// Resident memory, in bytes.
    pub memory_bytes: u64,
    /// CPU usage as a percentage of one core, since the previous read.
    pub cpu_percent: f64,
    /// How many threads the process has, when the platform reports them.
    pub threads: Option<usize>,
}

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
    /// How the retry loop is doing with each dispatch: how many times it has sent it again, and whether it stopped.
    ///
    /// ★ The loop owns the schedule; this is the part the interface reads. It is a separate map rather than the queue
    /// itself because the queue holds `Instant`s, a policy and cancellation, and none of that belongs in a DTO. The loop
    /// writes this map after every failed attempt and removes the entry when the intent is answered or ends.
    pub intent_retries: Arc<
        Mutex<std::collections::HashMap<synra_protocol::ids::IntentId, synra_ipc::IntentRetry>>,
    >,
    /// The plugins this shell can ask, when it has an engine.
    pub plugins: Option<Arc<Mutex<synra_core::dispatch::PluginSet>>>,
    /// What the startup load pass did: what runs, what did not, and why.
    ///
    /// ★ This is what makes "the plugin is not running" answerable. The state word on a card says `unavailable`, and
    /// without this that is all a user gets — the difference between a crash loop, a missing artifact and a plugin this
    /// build cannot run all look the same. The summary is the kernel's own record (`synra_core::plugin`), not a second
    /// derivation here, so the card and the startup report cannot disagree.
    pub plugin_loads: Mutex<synra_core::plugin::LoadedSummary>,
    /// Why no engine was built, when one was not.
    ///
    /// `plugins` being `None` covers two situations a user needs told apart: a build without an engine, and an engine
    /// that refused to start or could not enforce the limits this shell sets. The sentence is produced where the
    /// refusal happened, and the card carries it as the plugin's `stateReason`.
    pub engine_refusal: Option<String>,
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
    pub(crate) scanning: Arc<Mutex<Option<ScanTask>>>,
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
    /// A monotonic start, for the uptime metric.
    ///
    /// ★ Separate from `started_at_ms` because that one is the caller's wall clock — fixed in tests, and adjustable by
    /// the user between two reads. Uptime is the question "how long has this shell been up", and a clock correction must
    /// not make it jump backwards.
    pub started: std::time::Instant,
    /// The uptime last added to the counter, so each read adds only the delta.
    pub uptime_reported: Mutex<u64>,
    /// This process's id, so the sample is this process rather than whichever one the system lists first.
    process_pid: sysinfo::Pid,
    /// The system handle kept between reads: CPU usage is the delta between two refreshes, and a fresh handle would
    /// report zero on every read.
    process_system: Mutex<sysinfo::System>,

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
    pub registry_cache:
        Arc<Mutex<std::collections::HashMap<String, crate::commands::registry::CachedDocument>>>,
}

/// One plugin page this shell opened, as the window that opened it knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedUi {
    /// Committed authority this particular document was opened against.
    pub authority: synra_store::PluginAuthority,
    /// The plugin the page belongs to.
    pub plugin_id: synra_protocol::ids::PluginId,
    /// The version it was opened for, which validation compares against the session's.
    pub version: String,
    /// The token's nonce, as issued to this page.
    pub nonce: u128,
    /// Whether the page has said it is ready. A page that never does is a page that never drew.
    pub ready: bool,
}

/// Points the store's write timer at the shell's recorder.
///
/// # Why this is a named function
///
/// Because the metric's name and its label are one decision, and a test that wants to see a write counted should call the
/// same function the shell calls — not build a second histogram with the same name and hope the two agree. The daemon has
/// the same function over the same store hook; the hosts differ in where their recorder comes from, not in what a write
/// reports.
pub fn observe_db_writes(db: &mut SynraDb, metrics: &Arc<synra_observe::metrics::RingRecorder>) {
    let recorder = Arc::clone(metrics);
    db.observe_writes(Arc::new(move |kind, elapsed| {
        recorder.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "db_write_duration_us",
                [("kind".to_owned(), kind.to_owned())],
            ),
            elapsed.as_secs_f64() * 1_000_000.0,
        );
    }));
}

impl AppState {
    /// A state over an already-open database, with no plugin host.
    ///
    /// # Errors
    ///
    /// [`IpcError`] when the identity cannot be read or written. A shell with no identity has nothing to show a user
    /// and nothing to present to a peer, so this is a start-up refusal rather than a degraded mode.
    pub fn new(
        mut db: SynraDb,
        paths: Paths,
        platform: &str,
        now_ms: i64,
    ) -> Result<Self, IpcError> {
        let started = std::time::Instant::now();
        let identity = Identity::load(&db, platform, now_ms).map_err(internal)?;
        // Written back because `load` generates one on a fresh installation, and a generated identity that is only in
        // memory is a device that appears new to every peer on every start — which loses every pairing, silently.
        identity.store(&db, now_ms).map_err(internal)?;
        // ★ A previous run's uninstall may have left an artifact directory behind: a loaded engine held its wasm file
        // open, the row was already gone, and the user was told. This is the moment to finish it — the old process is
        // gone and no plugin has been loaded. The sandbox is not swept: data kept on purpose belongs to the user.
        if let Ok(rows) = db.plugins() {
            let installed: Vec<String> = rows.into_iter().map(|row| row.plugin_id).collect();
            match synra_store::PluginFiles::new(paths.clone()).sweep_orphans(&installed) {
                Ok(sweep) if !sweep.removed.is_empty() => eprintln!(
                    "synra: removed {} leftover plugin director{} ({} bytes)",
                    sweep.removed.len(),
                    if sweep.removed.len() == 1 { "y" } else { "ies" },
                    sweep.bytes_freed
                ),
                Ok(sweep) => {
                    for path in sweep.leftovers {
                        eprintln!("synra: could not remove {}", path.display());
                    }
                }
                Err(error) => eprintln!("synra: the plugin directory could not be swept: {error}"),
            }
        }
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
        // ★ The store reports every write's duration to this recorder. Installed once, where the database is owned, rather
        // than at each call site — the store's write methods are the one funnel every write goes through.
        observe_db_writes(&mut db, &metrics);

        // ★ The board survives a restart, and it did not before: a window that closed forgot every intent — including a
        // remote action a peer was still waiting on, so the peer retried and the action ran twice. The mapping is the kernel's
        // (`synra_core::intent::persist`), the same one the daemon writes; this only decides what a failure means.
        let mut board = Board::new();
        match synra_core::intent::persist::restore(&mut board, &db) {
            Ok(report) => {
                if report.restored > 0 {
                    eprintln!(
                        "synra: restored {} intent(s) from the last run",
                        report.restored
                    );
                }
                for (intent, why) in report.unreadable {
                    eprintln!(
                        "synra: stored intent {intent} could not be read back and was skipped: {why}"
                    );
                }
                for intent in report.duplicates {
                    eprintln!("synra: intent {intent} was restored twice");
                }
            }
            // A host that refused to start over unreadable rows would turn one bad document into the loss of every intent
            // behind it — including the ones a peer is waiting on. The absence is named rather than silent.
            Err(error) => eprintln!("synra: the stored intents could not be read: {error}"),
        }

        // ★ The process facts are kept between reads rather than rebuilt: `sysinfo` computes CPU usage as a delta between
        // two refreshes, so a fresh `System` would report zero on every panel poll. The first refresh here is the baseline.
        let process_pid = sysinfo::Pid::from_u32(std::process::id());
        let mut process_system = sysinfo::System::new();
        let _ = process_system
            .refresh_processes(sysinfo::ProcessesToUpdate::Some(&[process_pid]), true);

        let state = Self {
            process_pid,
            process_system: Mutex::new(process_system),
            host: shared(Host::new(identity.clone(), peer).with_metrics(Arc::clone(&metrics))),
            db: Arc::new(Mutex::new(db)),
            paths,
            identity: Arc::new(Mutex::new(identity)),
            board: Arc::new(Mutex::new(board)),
            intent_retries: Arc::new(Mutex::new(std::collections::HashMap::new())),
            plugins: None,
            plugin_loads: Mutex::new(synra_core::plugin::LoadedSummary::default()),
            engine_refusal: None,
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
            started: std::time::Instant::now(),
            uptime_reported: Mutex::new(0),
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
        // nothing in this shell swept them. The retention window is configured (`audit.retentionDays`), defaulting to the
        // design's seven days; see `prune_audit`.
        crate::commands::plugins::prune_audit(&state, now_ms);
        // ★ The intents table gets the same treatment, and for the same reason: a row is written on every change, so a shell
        // that restores without pruning grows a history it never reads. The window is the design's "24 hours after they
        // finish" (`17 §6`), and it is one constant in the kernel because the daemon sweeps the same table.
        //
        // ★ Before the sweep, the other half of a restored board: a `Dispatching` intent is work whose answer is still coming —
        // but only for as long as the far side's late queue keeps it. Once that window has passed no answer can arrive, and a
        // row that says "running" for ever is both wrong on the home page and uncollectable (the sweep only takes finished
        // intents). The kernel decides; this writes the rows so the decision survives the next start too.
        let expired = {
            let mut board = state.board.lock();
            synra_core::intent::persist::expire_unanswered(&mut board, now_ms)
        };
        for intent in &expired {
            crate::commands::intent::checkpoint(&state, *intent);
        }
        if !expired.is_empty() {
            eprintln!(
                "synra: {} dispatched intent(s) ended without an answer",
                expired.len()
            );
        }
        match synra_core::intent::persist::collect(
            &state.db(),
            synra_core::intent::persist::INTENT_RETENTION_MS,
            now_ms,
        ) {
            Ok(0) => {}
            Ok(removed) => {
                eprintln!("synra: removed {removed} finished intent(s) past their retention")
            }
            Err(error) => eprintln!("synra: finished intents could not be collected: {error}"),
        }
        // ★ What it cost to get here — the shell's own half of M4's cold-start budget. Recorded on the recorder the
        // performance panel reads, so "cold start < 500 ms" can be checked against a number rather than a stopwatch.
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "startup_duration_ms",
                [("phase".to_owned(), "state".to_owned())],
            ),
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        Ok(state)
    }

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
    fn constructing_the_state_records_how_long_it_took() {
        // ★ M4 asks for a cold start under 500 ms. A budget that is never written down on the machine is a sentence, not a
        // test; this is the number the performance panel reads.
        let state = crate::testing::state();

        assert!(
            state
                .metrics
                .snapshot()
                .get("startup_duration_ms{phase=state}")
                .is_some(),
            "the state's own construction time is not recorded"
        );
    }

    #[test]
    fn the_shells_host_counts_into_the_shells_recorder() {
        // ★ The host had no recorder before this: `Host::with_metrics` existed for the daemon, and the window built its
        // host without one — so the frame and link counters were zero in the only host a desktop user runs. The counter
        // is public precisely so a host can be shown to have one; the assertion is on the recorder the rest of the shell
        // reads (`diag_metrics`), not on a private field.
        let state = crate::testing::state();

        state.host.lock().count_frame_sent("kernel.test");

        assert_eq!(
            state
                .metrics
                .snapshot()
                .get("frames_sent_total{event=kernel.test}"),
            Some(&Value::Counter(1)),
            "the host's frame counter is the shell's recorder"
        );
    }

    #[test]
    fn a_store_write_reaches_the_shells_recorder() {
        // ★ The store has no recorder of its own: it reports a write to the host that installed an observer. This is the
        // shell's install, checked through the recorder `diag_metrics` reads — a write the user waits on should be in the
        // sample the performance panel shows.
        let state = crate::testing::state();

        state
            .db()
            .set_config("test.key", &serde_json::json!(1), 1)
            .expect("the write is made");

        assert!(
            state
                .metrics
                .snapshot()
                .get("db_write_duration_us{kind=config}")
                .is_some(),
            "the store's write was not counted by the shell's recorder"
        );
    }

    #[test]
    fn uptime_is_added_as_a_delta_rather_than_the_whole_value_again() {
        // ★ A counter set to the whole uptime on every read would grow with the number of refreshes, not with time. The
        // second sample says "four more seconds have passed", so the total is 14, not 24 — an assertion that only checked
        // one read would pass whichever way the arithmetic went.
        let state = crate::testing::state();

        state.record_uptime(10);
        state.record_uptime(14);

        assert_eq!(
            state.metrics.snapshot().get("uptime_seconds"),
            Some(&Value::Counter(14))
        );
    }
}
