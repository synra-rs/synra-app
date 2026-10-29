//! The devices this one can see.
//!
//! # The order the list is in
//!
//! Paired devices first, then by name, then by identifier. Paired first because that is the order a user reads the list
//! in — the devices they can already send to are what the screen is for, and the ones they have to pair with next are
//! what they are shopping for. Then the name, so the order is the same on every device that has seen the same set, and
//! then the identifier, so the order is *total*: two devices with the same name still agree on which comes first.
//!
//! # What is not here, and why
//!
//! `devices_connect` and `devices_disconnect` are absent from this stage. They are link-layer operations, the kernel has
//! no API for opening a session to a named device, and the only way to fake one — returning `Ok(())` without connecting —
//! is exactly the failure this module's own header forbids: an answer that says something happened when nothing did. They
//! arrive with the session commands, against a real [`Host`] API.

use std::time::{Duration, Instant};

use synra_core::supervisor::Cancellation;
use synra_ipc::{DeviceCard, DevicesSnapshot, IpcError, ScanOptions, ScanState};
use synra_protocol::DeviceId;

use crate::state::{AppState, ScanTask, internal};

/// How long a scan runs, from what the caller asked for.
///
/// # Why the protocol's budgets are both the default and the ceiling
///
/// Because a scan is a broadcast to every machine on the network, so how long one is allowed to run is the protocol's to
/// set and not the caller's: `SCAN_TOTAL` is the longest, and `SCAN_PASSIVE` is what "look now" means when the caller does
/// not say how long. A caller may ask for less and nothing may ask for more — the request arrives from a local process and
/// is unauthenticated input. (The shell used to carry its own thirty-second ceiling, from when it answered scans it had
/// not run; the protocol's is lower, which makes the shell's a second opinion nothing consults.)
fn scan_budget(opts: &ScanOptions) -> Duration {
    let asked = opts
        .timeout_ms
        .map_or(synra_protocol::limits::SCAN_PASSIVE, Duration::from_millis);
    asked.min(synra_protocol::limits::SCAN_TOTAL)
}

/// Everything the devices screen shows, in one call.
///
/// One call rather than three, because a screen assembled from three calls renders three inconsistent states — a list
/// with no name beside it, a scan that started after the list was fetched — and the user sees that as the app flickering.
///
/// # Errors
///
/// [`IpcError::Internal`] when the device records cannot be read, **or when one of them holds an identifier that is not
/// an identifier**. The second is the interesting one: a row whose device id does not parse is a corrupted database, and
/// answering with the rest of the list would present a short list as if those devices did not exist.
pub fn devices_snapshot(state: &AppState) -> Result<DevicesSnapshot, IpcError> {
    // ★ One guard for both reads, dropped before any card is built — building reads the manifest off disk, and holding a
    // database lock across file I/O would serialise every other command behind the filesystem.
    let (paired, seen) = {
        let db = state.db();
        (
            db.paired_devices().map_err(internal)?,
            db.seen_devices().map_err(internal)?,
        )
    };

    let mut cards: Vec<DeviceCard> = paired
        .iter()
        .map(|row| card_from_paired(&state.host, row))
        .collect::<Result<_, _>>()?;
    // ★ Seen-but-unpaired devices are added here rather than by the interface.
    //
    // The interface cannot know which seen devices are already paired — that is the kernel's record — so a list built
    // by the interface either shows a paired device twice or needs a second call to find out which are which. Both are
    // worse than one call that already knows.
    for row in &seen {
        let card = card_from_seen(row)?;
        if cards
            .iter()
            .any(|existing| existing.device_id == card.device_id)
        {
            continue;
        }
        cards.push(card);
    }
    cards.sort_by(|left, right| {
        right
            .paired
            .cmp(&left.paired)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.device_id.cmp(&right.device_id))
    });

    let scanning = state
        .scanning
        .lock()
        .as_ref()
        .map(|task| task.snapshot.clone());
    Ok(DevicesSnapshot {
        identity: state.identity_snapshot(crate::KERNEL_VERSION),
        devices: cards,
        scanning,
    })
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

fn scan_with(
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
fn announce_cards(state: &AppState) {
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

/// Forgets a device, and its pairing with it.
///
/// # Errors
///
/// * [`IpcError::InvalidArgument`] when the identifier does not parse.
/// * [`IpcError::Internal`] when the unpairing cannot be written.
pub fn devices_forget(state: &AppState, device_id: &str) -> Result<(), IpcError> {
    let parsed = parse_from_interface(device_id)?;
    // Serialise with acceptance: a pending response cannot re-add the preference after unpairing. Remove the preference
    // first and propagate failures. If unpairing then fails, the device stays paired but asks again, and that safer partial
    // result is announced rather than hidden. No host/plugin work runs while the database is held.
    let (unpaired, changed) = {
        let db = state.db();
        let changed = crate::commands::transfers::forget_sender(&db, device_id)?;
        (db.unpair_device(device_id).map_err(internal), changed)
    };
    if changed {
        super::config::announce_config(state);
    }
    unpaired?;
    state.host.lock().went_offline(parsed, state.started_at_ms);
    // ★ The window learns the device is gone without asking. What a page drew is a snapshot; a device forgotten here
    // would otherwise stay on the list until the page was reopened — with a pairing prompt beside it.
    state.publish(synra_ipc::event::KernelEvent::DeviceRemoved { device_id: parsed });
    Ok(())
}

/// A card for a device that was seen and never paired.
///
/// # Errors
///
/// [`IpcError::Internal`] when the stored identifier does not parse, for the reason [`devices_snapshot`] gives: a
/// corrupted row must not become a missing card.
fn card_from_seen(row: &synra_store::SeenDeviceRow) -> Result<DeviceCard, IpcError> {
    let device_id = parse_from_store(row.device_id.as_str())?;
    Ok(DeviceCard {
        device_id,
        display_name: row.display_name.clone(),
        platform: row.platform.clone(),
        online: true,
        paired: false,
        // A seen device that has never been paired is not blocked; blocking is a decision the user made, and a row that
        // has one is a paired row that was left in place deliberately.
        blocked: false,
        last_seen_ms: row.last_seen_ms,
        // ★ No address for a seen device.
        //
        // The store keeps an address for a paired device because that is where it is *reachable*; for a device seen once
        // over discovery, the address in that announcement is where it was then, which is not a promise about now.
        // Showing it would offer a device the user cannot reach, on a card that looks exactly as usable as a paired one.
        // Absent is the honest answer, and the interface is already written to omit the field.
        address: None,
        transfer: None,
    })
}

/// The card for one device, looked up in the record.
///
/// # Why this exists rather than a call to [`devices_snapshot`]
///
/// Because the caller that needs it is the **discovery worker's** announcement, and it has no `AppState` to give — only the
/// database, the session registry and the identifier it just recorded. Building the card from the same two functions the
/// snapshot uses is what keeps one description of a card instead of two.
///
/// # Errors
///
/// [`IpcError::Internal`] when a stored identifier does not parse, for the reason [`devices_snapshot`] gives.
#[cfg(feature = "tauri-shell")]
pub(crate) fn card_for(
    db: &std::sync::Arc<parking_lot::Mutex<synra_store::SynraDb>>,
    host: &synra_core::SharedHost,
    device_id: &str,
) -> Result<Option<DeviceCard>, IpcError> {
    // Locked, read, released before a card is built: `card_from_paired` takes the registry, and holding the database lock
    // across that is one more lock order for nobody's benefit.
    let (paired, seen) = {
        let db = db.lock();
        (
            db.paired_devices().map_err(internal)?,
            db.seen_devices().map_err(internal)?,
        )
    };

    if let Some(row) = paired.iter().find(|row| row.device_id == device_id) {
        return card_from_paired(host, row).map(Some);
    }
    match seen.iter().find(|row| row.device_id == device_id) {
        Some(row) => card_from_seen(row).map(Some),
        None => Ok(None),
    }
}

/// A card for a device this kernel has paired with.
///
/// # Errors
///
/// [`IpcError::Internal`] when the stored identifier does not parse. See [`devices_snapshot`]: a corrupted row must not
/// become a missing card.
fn card_from_paired(
    host: &synra_core::SharedHost,
    row: &synra_store::DeviceRow,
) -> Result<DeviceCard, IpcError> {
    let device_id = parse_from_store(row.device_id.as_str())?;
    Ok(DeviceCard {
        device_id,
        display_name: row.display_name.clone(),
        platform: row.platform.clone(),
        // A paired device with no open session is offline rather than absent — the two answer different questions
        // ("can I send to this now" against "have I ever heard of it"), and the interface renders them differently.
        //
        // ★ This line said `true` for several rounds while the sentence above it said "offline", and the sentence was
        // right. Every paired device was reported online, so the dashboard showed a green dot for a laptop that had been
        // shut for a week, and the first the user heard about it was a send that failed. The registry is what knows:
        // `live_for` answers "is there a session with this peer now", which is exactly the question the card asks.
        online: host.lock().sessions().live_for(device_id).is_some(),
        paired: true,
        blocked: row.blocked,
        last_seen_ms: row.last_seen_ms,
        address: row.address.clone(),
        transfer: None,
    })
}

/// Parses a device identifier the interface sent.
///
/// # Errors
///
/// [`IpcError::InvalidArgument`], because the caller sent it and the caller can fix it.
fn parse_from_interface(device_id: &str) -> Result<DeviceId, IpcError> {
    device_id.parse::<DeviceId>().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"deviceId": device_id, "detail": error.to_string()}))
    })
}

/// Parses a device identifier the kernel stored.
///
/// # Errors
///
/// [`IpcError::Internal`]. A row that does not parse is a corrupted database, not a bad request — and reporting it as
/// `invalid_argument` would send the user looking at their own input for a fault that is on disk.
fn parse_from_store(device_id: &str) -> Result<DeviceId, IpcError> {
    device_id.parse::<DeviceId>().map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::Internal)
            .with_params(serde_json::json!({"deviceId": device_id, "detail": error.to_string()}))
    })
}

#[cfg(test)]
mod tests;
