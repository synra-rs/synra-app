use super::devices_snapshot;
use crate::state::{AppState, ScanTask};
use std::time::{Duration, Instant};
use synra_core::supervisor::Cancellation;
use synra_ipc::{IpcError, ScanOptions, ScanState};

/// How long a scan runs, from what the caller asked for.
///
/// # Why the protocol's budgets are both the default and the ceiling
///
/// Because a scan is a broadcast to every machine on the network, so how long one is allowed to run is the protocol's to
/// set and not the caller's: `SCAN_TOTAL` is the longest, and `SCAN_PASSIVE` is what "look now" means when the caller does
/// not say how long. A caller may ask for less and nothing may ask for more — the request arrives from a local process and
/// is unauthenticated input. (The shell used to carry its own thirty-second ceiling, from when it answered scans it had
/// not run; the protocol's is lower, which makes the shell's a second opinion nothing consults.)
pub(super) fn scan_budget(opts: &ScanOptions) -> Duration {
    let asked = opts
        .timeout_ms
        .map_or(synra_protocol::limits::SCAN_PASSIVE, Duration::from_millis);
    asked.min(synra_protocol::limits::SCAN_TOTAL)
}

/// Starts a scan and answers with its identifier.
///
/// # ★ This command used to say it could not scan, and that was true twice over
///
/// The first version recorded a scan nobody performed: a state carrying `elapsed_ms: 0, found: 0` that **nothing updated**,
/// so the interface drew a bar that could never move. The second refused outright, with a reason that named the missing
/// half: discovery belongs to the process that accepts connections, the daemon was running one, and there was no way to ask
/// it for a burst. "Found nothing" was true; "is scanning" was not, and a refusal was the honest answer while there was no
/// scanner.
///
/// Now there is one, and it is this process's: [`crate::discovery`] binds the discovery sockets, runs the worker that
/// `synra-core` already implements, and writes what it hears into **this** process's `devices_seen` — the table
/// [`devices_snapshot`] reads. That is the design's own arrangement ([05 §2.1](../../../design-docs/05-system-architecture.md)
/// puts `DiscoveryService` inside the desktop application), and it is also the only arrangement that shows the user
/// anything: the daemon keeps a different data directory, so a scan it ran would write into a table this window never reads.
///
/// # Errors
///
/// [`IpcError::Unsupported`] when this window could not bind the discovery sockets — no network, a firewall, a sandbox —
/// carrying the reason. The interface falls back to the cached read, which answers what is already known.
///
/// # What it leaves behind
///
/// A cached scan leaves nothing: it reads, it has finished before it answers, and `scanning` means *in progress*. A network
/// scan records `scanning` **while it runs**, with the budget it is waiting out, and clears it before answering — so a
/// snapshot taken during the wait says a scan is under way, and the one taken after it does not.
pub fn devices_scan_start(state: &AppState, opts: &ScanOptions) -> Result<String, IpcError> {
    scan_with(state, opts, crate::discovery::Discovery::scan)
}

pub(super) fn scan_with(
    state: &AppState,
    opts: &ScanOptions,
    scan: impl FnOnce(&crate::discovery::Discovery, Duration, &Cancellation),
) -> Result<String, IpcError> {
    let scan_id = state.next_scan_id();

    // A cached scan reads what is already known: no sockets, nothing to wait for, and no state to leave behind.
    if opts.cached_only {
        return Ok(scan_id);
    }

    // ★ `Some(0)` is the protocol's way of saying "answer from what you already know" — the same thing `cached_only` says —
    // so nothing is published and nobody waits. The daemon reads its own `scan_ms` the same way.
    let budget = scan_budget(opts);
    if budget.is_zero() {
        return Ok(scan_id);
    }

    let Some(discovery) = &state.discovery else {
        // ★ Counted before the refusal: "why can this device not find anything" is a question the metric answers, and a
        // scan that never ran is exactly the case a user hits on a machine with no usable network.
        count_scan(state, "unavailable");
        return Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported).with_params(serde_json::json!({
            "reason": "this window could not bind the discovery sockets, so it cannot look for devices: a machine with \
                no network, a firewall and a sandbox all produce this. The devices already known are still listed — ask \
                for a cached scan to read them.",
        })));
    };

    // Zero elapsed with the whole budget ahead: this is the state of the scan **as it starts**, and the number that
    // matters is `total_ms` — a caller that got it knows the scan is real and bounded.
    let running = ScanState {
        scan_id: scan_id.clone(),
        elapsed_ms: 0,
        total_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
        found: 0,
    };
    let cancellation = {
        let mut scanning = state.scanning.lock();
        if let Some(active) = scanning.as_ref() {
            return Err(IpcError::unknown(synra_protocol::ErrorCode::RateLimited)
                .with_params(serde_json::json!({"running": active.snapshot.scan_id})));
        }
        let task = ScanTask::new(running.clone());
        let cancellation = task.cancellation.clone();
        *scanning = Some(task);
        cancellation
    };
    let mut completion = ScanCompletion {
        state,
        scan_id: &scan_id,
        started: Instant::now(),
        found: 0,
        completed: false,
        cancellation: &cancellation,
    };
    // ★ The window learns the scan started without asking. The comment here used to say this shell published no kernel
    // events, and while that was true the Scan button had to poll to draw anything. Completion publishes another event
    // after clearing the task; the global event handler then reads the authoritative snapshot.
    state.publish(synra_ipc::event::KernelEvent::ScanUpdated(Box::new(
        running,
    )));
    // ★ Reset before the scan and read after it: the counter is "what this scan heard", not "what this process has ever
    // heard", and a value left over from the previous scan would make the second one look more successful than it was.
    discovery.reset_sightings();
    scan(discovery, budget, &cancellation);
    completion.found = discovery.sightings();
    completion.completed = true;
    // Discovery has released its demand window. Only now can a new task own the scanner.
    drop(completion);
    announce_cards(state);

    Ok(scan_id)
}

/// Clears only this task and notifies readers, even if its worker unwinds.
struct ScanCompletion<'a> {
    state: &'a AppState,
    scan_id: &'a str,
    started: Instant,
    found: u64,
    completed: bool,
    cancellation: &'a Cancellation,
}

impl Drop for ScanCompletion<'_> {
    fn drop(&mut self) {
        let finished = {
            let mut scanning = self.state.scanning.lock();
            if scanning
                .as_ref()
                .is_none_or(|task| task.snapshot.scan_id != self.scan_id)
            {
                return;
            }
            scanning.take().map(|task| ScanState {
                elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                found: u32::try_from(self.found).unwrap_or(u32::MAX),
                ..task.snapshot
            })
        };
        if let Some(finished) = finished {
            let result = if !self.completed {
                "failed"
            } else if self.cancellation.is_cancelled() {
                "cancelled"
            } else {
                "ran"
            };
            count_scan(self.state, result);
            observe_candidates(self.state, self.found);
            self.state.metrics.histogram_observe(
                &synra_observe::metrics::SeriesKey::new("scan_duration_ms", []),
                self.started.elapsed().as_secs_f64() * 1_000.0,
            );
            self.state
                .publish(synra_ipc::event::KernelEvent::ScanUpdated(Box::new(
                    finished,
                )));
        }
    }
}

/// Counts one scan by how it ended, with the daemon's own label spelling (`ran` / `unavailable`).
fn count_scan(state: &AppState, result: &str) {
    state
        .metrics
        .counter_inc(&synra_observe::metrics::SeriesKey::new(
            "scans_total",
            [("result".to_owned(), result.to_owned())],
        ));
}

/// Records how many devices one scan found.
///
/// A histogram rather than a counter: the interesting question is the distribution ("this network usually has three"),
/// which a total cannot answer.
#[allow(clippy::cast_precision_loss)]
fn observe_candidates(state: &AppState, found: u64) {
    state.metrics.histogram_observe(
        &synra_observe::metrics::SeriesKey::new("candidates_found", []),
        found as f64,
    );
}

/// Announces every device the kernel currently knows, one card at a time.
///
/// # Why it is a function rather than four lines in the scan
///
/// Because the scan's happy path cannot be tested without binding sockets, and this is the part worth a test: a scan ends
/// and the window learns what it produced. Extracted, a test can call it on a fixture with one paired device and assert the
/// event — no network, no timing.
///
/// # Why one event per device
///
/// The interface applies each card to the store it already has, so a device that appeared, one that went offline and one
/// that changed its name are three different facts; a single "the list changed" would collapse them into a re-read of
/// everything. The cards come from `devices_snapshot`, so the card in the event is the card in the next list.
pub(super) fn announce_cards(state: &AppState) {
    if let Ok(snapshot) = devices_snapshot(state) {
        for card in snapshot.devices {
            state.publish(synra_ipc::event::KernelEvent::DeviceCardUpdated(Box::new(
                card,
            )));
        }
    }
}

/// Requests cancellation and wakes the admitted scan. Its owner publishes actual completion.
///
/// # Errors
///
/// [`IpcError::InvalidArgument`] when the identifier is not the scan in progress, carrying the running identifier in the
/// refusal's parameters so a stale interface can correct itself.
///
/// Not silently ignored, because a cancel naming the wrong scan nearly always means the interface's state is out of
/// step with the kernel's — and a shrug there looks exactly like it worked. `InvalidArgument` rather than a new wire
/// code: a scan identifier is a shell-local thing, and adding a protocol code for it would put a user-interface concept
/// into the vocabulary every peer on every device has to understand. What it is, is an argument that did not match
/// reality.
pub fn devices_scan_cancel(state: &AppState, scan_id: &str) -> Result<(), IpcError> {
    // ★ Scoped to the decision, not to the function.
    //
    // `devices_snapshot` reads this same table on every screen refresh, and a cancel that held the lock while building
    // its refusal would make a refresh behind it wait — which is how a settings screen ends up stuttering every time
    // someone presses stop.
    let cancellation = {
        let scanning = state.scanning.lock();
        let running = scanning.as_ref().ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"scanId": scan_id}))
        })?;
        if running.snapshot.scan_id != scan_id {
            return Err(
                IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
                    serde_json::json!({"scanId": scan_id, "running": running.snapshot.scan_id}),
                ),
            );
        }
        let cancellation = running.cancellation.clone();
        drop(scanning);
        cancellation
    };
    cancellation.cancel();
    Ok(())
}
