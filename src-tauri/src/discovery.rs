//! This window's own discovery: the sockets, the worker, and the deadline a scan publishes.
//!
//! # Why the shell discovers rather than asking the daemon
//!
//! Three reasons, and the second is the one that settles it:
//!
//! 1. The design says so. [05 §2.1](../../../design-docs/05-system-architecture.md) puts `DiscoveryService` **inside the
//!    desktop application**, in the same process as the window; the daemon is the *headless* host. A desktop that needed
//!    a second program running beside it would be one whose device list is empty until somebody starts that program.
//! 2. The two hosts keep **different data directories**, so a scan run by the daemon is a scan this window cannot see. The
//!    window's is Tauri's `app_data_dir` (`%APPDATA%\com.synra.app` on Windows); the daemon's is `SYNRA_DATA_DIR`, or a
//!    directory under the temporary directory when that is unset. The daemon writes what it hears into **its own**
//!    `devices_seen`, and the table the devices screen reads is this process's — so "found three devices and shows none"
//!    is what a delegated scan would produce, which is worse than a refusal.
//! 3. `devices_scan_start` is the interface's command, and an implementation that needs another process would be one whose
//!    failure mode ("the background service is not running") has nothing to do with the question the user asked.
//!
//! # What it announces as its port, and why it is the discovery port
//!
//! ★ This build has **no transport**: the window accepts no connections, so there is no port at which this device can be
//! reached — and the wire format will not let it say so, because [`DeviceAnnouncement::validate`] refuses `port: 0`. So the
//! announcement names the one port this process really does bind: the discovery responder. A peer that records this device
//! and later dials it reaches a discovery responder rather than a transport, which is a fact about this build rather than
//! an address nobody holds. When the shell gains its transport half, this becomes that port and this paragraph goes.
//!
//! [`DeviceAnnouncement::validate`]: synra_protocol::discovery::DeviceAnnouncement::validate

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use synra_core::discovery::Candidate;
use synra_core::supervisor::{Supervisor, TaskId, TaskSpec, run_task};
use synra_core::{DiscoveryService, QUERY_INTERVAL, SharedHost};
use synra_protocol::discovery::DeviceAnnouncement;
use synra_protocol::limits::SCAN_PASSIVE;
use synra_protocol::session::PlatformKind;
use synra_store::{Seen, SynraDb};

/// What the worker calls for each device it managed to record.
///
/// A function rather than a bus handle: the worker has a **sighting** and an event needs a **card**, and building one needs
/// the record and the session registry. Handing those to the socket loop would put two more pieces of the shell inside it,
/// so the loop reports what it saw and the host decides what to say.
pub type Announce = Arc<dyn Fn(&str) + Send + Sync>;

/// The device identifiers one scan heard, deduplicated.
///
/// Shared between the worker (which inserts) and the command (which resets and reads): a device announces more than once
/// inside the protocol's scan window, and `candidates_found` asks how many devices answered rather than how many packets
/// arrived.
pub(crate) type Sightings = Arc<Mutex<std::collections::HashSet<String>>>;

/// Records one device the worker accepted into the scan's set.
///
/// A free function because the worker closure holds the [`Sightings`] handle rather than a [`Discovery`]: the worker starts
/// before the struct exists, and handing it the whole struct would be handing it the sockets it is already running.
pub(crate) fn note_sighting(sightings: &Sightings, device_id: &str) {
    sightings.lock().insert(device_id.to_owned());
}

/// How often the wait loop looks at the clock.
///
/// Not a deadline for anything: the whole wait is bounded by the protocol's scan window, and this is only how often the
/// loop comes back round. The daemon's own scan waits in the same slices.
const WAIT_SLICE: Duration = Duration::from_millis(20);

/// The discovery worker, and the deadline a caller publishes to ask it to look now.
///
/// # Why the field is the deadline rather than a method on the worker
///
/// Because the worker owns the sockets and runs on its own thread, so the only thing a caller on another thread *can*
/// share with it is a value it reads each turn. `synra_core`'s worker already reads exactly this one (`on_demand`), and the
/// daemon publishes to the same field for the same reason — a scan through the window and a scan through the CLI are the
/// same scan, not two implementations of one idea.
pub struct Discovery {
    /// The sockets, owned by the thread `start` spawns.
    service: Arc<Mutex<DiscoveryService>>,
    /// When a caller asked for a scan, as a wall-clock millisecond the worker compares against now.
    on_demand: Arc<AtomicI64>,
    /// What the most recent scan heard, so the command can report it. Reset by the caller before it asks for a scan.
    ///
    /// ★ A **set** rather than a count, because a device announces more than once inside one scan window and the question
    /// `candidates_found` asks is "how many devices answered", not "how many packets arrived". The daemon reads the same
    /// question from `host.discovery().len()`, which is also a set.
    sightings: Sightings,
    /// The supervisor's shutdown token, so a deliberate exit can stop the worker.
    ///
    /// ★ The token rather than the handle: `run_task` joins on the token, and the supervisor is what waits when a caller
    /// wants a report on the shutdown. This shell has no such report, so it keeps the one value the worker actually reads.
    stop: synra_core::supervisor::Cancellation,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Discovery").field("service", &self.service).finish_non_exhaustive()
    }
}

impl Discovery {
    /// Binds the sockets and starts the worker.
    ///
    /// # Why the responder port is a parameter
    ///
    /// For the reason [`DiscoveryService::bind_on`] takes one: a test needs two kernels on one host, and the default is
    /// the protocol's constant. A window that listened somewhere else could not be found by anything that did not already
    /// know where to look — so the shell passes the constant and only a test moves it.
    ///
    /// # Errors
    ///
    /// A message naming what stopped it. A machine with no network, a firewall and a sandbox all produce the same thing,
    /// and none of them is a reason to lose the window — the caller reports it and carries on without a scan.
    pub fn start(
        ours: DeviceAnnouncement,
        responder_port: u16,
        db: Arc<Mutex<SynraDb>>,
        host: SharedHost,
        started_at_ms: i64,
        announce: Announce,
    ) -> Result<Self, String> {
        let service = DiscoveryService::bind_on(ours, responder_port, QUERY_INTERVAL)
            .map_err(|error| format!("the discovery sockets could not be bound: {error}"))?;
        let service = Arc::new(Mutex::new(service));
        let on_demand = Arc::new(AtomicI64::new(0));
        // ★ The supervisor is not kept, and that is a decision rather than an omission.
        //
        // The worker runs for the process's lifetime and `run_task` clones the supervisor into the thread that runs it, so
        // nothing here has to hold one alive. What a *handle* would be for is a report on the task — and this shell's
        // diagnostics do not report on tasks yet. A field nobody reads is a field that drifts, so the handle goes with the
        // report that would use it.
        //
        // The shutdown token **is** kept now: a deliberate exit asks the worker to stop before the process ends, which is
        // what makes the sockets close and the last sighting written rather than cut off mid-turn.
        let supervisor = Arc::new(Supervisor::new(started_at_ms));
        let stop = supervisor.cancellation();
        // Registered before it is run, because `run_task` refuses a task its supervisor does not know — the registry doing
        // its job: a task that ran unregistered is one no shutdown report would ever mention. `TaskSpec::service` is the
        // ordinary criticality, which is what a loop that lives as long as the process is.
        supervisor
            .register(TaskSpec::service("discovery.udp"))
            .map_err(|error| format!("the discovery task could not be registered: {error}"))?;

        // ★ Registered and started in one call, and a failure to spawn is reported rather than ignored: a window that
        // believed it was discovering while no thread existed is the failure this module is about.
        let worker = Arc::clone(&service);
        let deadline = Arc::clone(&on_demand);
        let sightings: Sightings = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let counted = Arc::clone(&sightings);
        run_task(&supervisor, &TaskId::new("discovery.udp"), move |cancel| {
            synra_core::worker::run_recording(
                &worker,
                &host,
                &deadline,
                cancel,
                synra_store::sqlite::now_ms,
                // What was heard is written where the devices screen reads it. That write is the caller's job rather than
                // `synra-core`'s, deliberately: a crate that knows nothing about storage is what keeps a database out of
                // a socket loop.
                move |candidates| {
                    // ★ Every device the table accepted is announced, and only those: `record` skips a sighting whose write
                    // failed, so a device the kernel could not remember is not one the window is told about.
                    for device_id in record(&db, candidates) {
                        // The set deduplicates the repeats inside one scan; the same identifier twice is one device.
                        note_sighting(&counted, &device_id);
                        announce(&device_id);
                    }
                },
            );
            Ok(())
        })
        .map_err(|error| format!("the discovery worker did not start: {error}"))?;

        Ok(Self { service, on_demand, stop, sightings })
    }

    /// Asks the worker to stop, for a deliberate exit.
    ///
    /// The worker also stops when the process ends; asking first is what closes the sockets and lets the current turn
    /// finish its write, which is the difference between a shutdown and a disappearance.
    pub fn stop(&self) {
        self.stop.cancel();
    }

    /// Whether the worker has been asked to stop.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stop.is_cancelled()
    }

    /// The address the responder is bound to, for the log line that says whether this device can be found.
    ///
    /// # Errors
    ///
    /// A message when the socket cannot report it.
    pub fn responder_address(&self) -> Result<std::net::SocketAddr, String> {
        self.service.lock().responder_address().map_err(|error| error.to_string())
    }

    /// The deadline the worker reads, for a caller that wants to watch a scan happen.
    ///
    /// Exposed because a test has no other way to see that a scan published one: the command blocks, so by the time it
    /// answers the deadline has been cleared again.
    #[must_use]
    pub fn on_demand(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.on_demand)
    }

    /// Forgets what the previous scan heard, for the scan that is about to run.
    pub(crate) fn reset_sightings(&self) {
        self.sightings.lock().clear();
    }

    /// How many distinct devices the current (or last) scan recorded.
    pub(crate) fn sightings(&self) -> u64 {
        u64::try_from(self.sightings.lock().len()).unwrap_or(u64::MAX)
    }

    /// Asks the worker to look now, and waits out the window the protocol gives one scan.
    ///
    /// # Why this waits rather than answering at once
    ///
    /// Because the answer to "scan now" has to be a list that includes what the scan heard. Publishing the deadline and
    /// returning would leave the caller to guess how long to wait — and the interface, which reads the table, would read
    /// it before the answers arrived. The wait is bounded by the protocol's own window (1.2 seconds) and not by the
    /// network, and the window is longer than the protocol's probe timeout, so the answers to the broadcast this triggers
    /// have arrived, been absorbed and been written down before this returns.
    pub fn scan(&self, budget: Duration) {
        // ★ The shell's clock is already the signed reading this deadline and the discovery table use, so nothing is
        // converted here. The daemon does convert (`millis_reading`), because its clock counts **unsigned** milliseconds —
        // two hosts, two clocks, one field, and the difference is worth a line rather than a copy of the daemon's code.
        let deadline = synra_store::sqlite::now_ms().saturating_add(as_millis(budget));
        self.on_demand.store(deadline, Ordering::Relaxed);

        let window = SCAN_PASSIVE.min(budget);
        let started = Instant::now();
        while started.elapsed() < window {
            std::thread::sleep(WAIT_SLICE);
        }

        // ★ Released rather than left to expire. The worker would notice within one tick of its own, and a scan that keeps
        // broadcasting after its caller was answered is a scan the user did not ask for — the daemon's own scan says the
        // same thing in the same place.
        self.on_demand.store(0, Ordering::Relaxed);
    }
}

/// Writes what discovery heard into `devices_seen`, which is the table the devices screen reads.
///
/// # Why a failure is reported and not retried
///
/// Because the next announcement refreshes the row anyway. A scan of a network full of devices would otherwise report the
/// same failure once per device per scan, and a log line repeated is a log line nobody reads.
/// # Returns
///
/// The identifiers it managed to record, so a caller that wants to announce them does not have to re-derive which of the
/// candidates landed — and so a device whose row was refused is not announced as if it had been.
pub(crate) fn record(db: &Arc<Mutex<SynraDb>>, candidates: &[Candidate]) -> Vec<String> {
    let mut recorded = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let sighting = Seen {
            device_id: candidate.device_id.to_string(),
            display_name: candidate.display_name.clone(),
            platform: platform_name(candidate.platform),
            seen_at_ms: candidate.last_seen_ms,
            source: candidate.address.to_string(),
        };
        if let Err(error) = db.lock().record_seen_device(&sighting) {
            eprintln!("synra: could not remember {}: {error}", sighting.device_id);
            continue;
        }
        recorded.push(sighting.device_id);
    }
    recorded
}

/// The platform string the device table stores.
///
/// The announcement's platform is an enum and the column is a string, and the enum's own name is the string the device
/// card compares against. A device that said nothing about itself is a desktop, which is the same default the daemon uses.
fn platform_name(platform: Option<PlatformKind>) -> String {
    format!("{:?}", platform.unwrap_or(PlatformKind::Desktop))
}

/// A duration as whole milliseconds, saturating rather than wrapping.
fn as_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
