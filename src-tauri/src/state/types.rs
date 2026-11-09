use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::Mutex;

use synra_core::intent::Board;
use synra_core::supervisor::Cancellation;
use synra_core::{Identity, SharedHost};
use synra_ipc::ScanState;
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
    /// Shared input session lifetime; pointer control is never inferred from pairing.
    pub input: crate::input::Input,
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
    pub(super) process_pid: sysinfo::Pid,
    /// The system handle kept between reads: CPU usage is the delta between two refreshes, and a fresh handle would
    /// report zero on every read.
    pub(super) process_system: Mutex<sysinfo::System>,

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
