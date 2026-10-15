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

use synra_ipc::{DeviceCard, DevicesSnapshot, IpcError, ScanOptions, ScanState};
use synra_protocol::DeviceId;

use crate::state::{AppState, internal};

/// The scan budget a request may ask for, in milliseconds.
///
/// The shell's own ceiling rather than the kernel's, because the shell is what renders a progress bar and a bar that
/// runs for a minute is one nobody waits for. A caller asking for longer gets this and a snapshot showing the elapsed
/// time, which is a smaller lie than a scan that never ends.
pub const SCAN_CEILING_MS: u64 = 30_000;

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
        (db.paired_devices().map_err(internal)?, db.seen_devices().map_err(internal)?)
    };

    let mut cards: Vec<DeviceCard> =
        paired.iter().map(|row| card_from_paired(state, row)).collect::<Result<_, _>>()?;
    // ★ Seen-but-unpaired devices are added here rather than by the interface.
    //
    // The interface cannot know which seen devices are already paired — that is the kernel's record — so a list built
    // by the interface either shows a paired device twice or needs a second call to find out which are which. Both are
    // worse than one call that already knows.
    for row in &seen {
        let device_id = parse_from_store(row.device_id.as_str())?;
        if cards.iter().any(|card| card.device_id == device_id) {
            continue;
        }
        cards.push(DeviceCard {
            device_id,
            display_name: row.display_name.clone(),
            platform: row.platform.clone(),
            online: true,
            paired: false,
            // A seen device that has never been paired is not blocked; blocking is a decision the user made, and a row
            // that has one is a paired row that was left in place deliberately.
            blocked: false,
            last_seen_ms: row.last_seen_ms,
            // ★ No address for a seen device.
            //
            // The store keeps an address for a paired device because that is where it is *reachable*; for a device seen
            // once over discovery, the address in that announcement is where it was then, which is not a promise about
            // now. Showing it would offer a device the user cannot reach, on a card that looks exactly as usable as a
            // paired one. Absent is the honest answer, and the interface is already written to omit the field.
            address: None,
            transfer: None,
        });
    }
    cards.sort_by(|left, right| {
        right
            .paired
            .cmp(&left.paired)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.device_id.cmp(&right.device_id))
    });

    let scanning = state.scanning.lock().clone();
    Ok(DevicesSnapshot {
        identity: state.identity_snapshot(crate::KERNEL_VERSION),
        devices: cards,
        scanning,
    })
}

/// Starts a scan and answers with its identifier.
///
/// # ★ Only the cached scan can be honoured, and this command used to claim otherwise
///
/// A scan that is not `cached_only` is refused, and the reason is worth writing down because the code that this replaced
/// argued the opposite and was wrong.
///
/// It said: *"a shell with no sockets is a normal configuration rather than a failure, so the scan is recorded anyway
/// and reports zero found — which is a true answer, not a stub."* The second half is where it goes wrong. "Found
/// nothing" is true; **"is scanning" is not**. The state was written once with `elapsed_ms: 0, found: 0` and updated by
/// nothing, so the interface drew a progress bar that could never move and a count that could never rise. That is an
/// invented answer, and this module\u2019s own rule is that a command answers or says why it cannot.
///
/// # Why the shell cannot scan at all
///
/// `synra_core::DiscoveryService` is in the library and the shell could bind one — but it would have to announce a port,
/// and the shell serves nothing on one. Discovery belongs to the process that accepts connections, which is the daemon,
/// and it is already running there: the daemon's worker lives for the process's lifetime and writes what it hears into
/// the same `devices_seen` table this shell reads. So the list is current without this command doing anything.
///
/// What is missing is the *on-demand burst* — asking the daemon to query now rather than wait for its next interval —
/// and there is no control request for it. Until one exists, "scan now" has no path, and a bar that fills for nothing is
/// worse than a refusal that says so.
///
/// # Errors
///
/// [`IpcError`] when an active scan was asked for. A `cached_only` scan is answered, because reading what is already
/// known needs no sockets and is exactly what the shell can do.
pub fn devices_scan_start(state: &AppState, opts: &ScanOptions) -> Result<String, IpcError> {
    if !opts.cached_only {
        return Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported).with_params(
            serde_json::json!({
                "reason": "this shell cannot scan: discovery belongs to the daemon, which is already \
                    discovering and writing what it hears into the same device list. What is missing is a \
                    control request asking it for an on-demand scan. Ask for a cached scan to see \
                    what is already known.",
            }),
        ));
    }
    // ★ `cached_only` finishes immediately, with nothing to wait for.
    //
    // A "show me what I already know" scan uses no sockets, so giving it a running state with a deadline would show a
    // progress bar that cannot progress. Zero is what a finished scan looks like, and it is true here.
    let scan_id = state.next_scan_id();
    *state.scanning.lock() = Some(ScanState {
        scan_id: scan_id.clone(),
        elapsed_ms: 0,
        total_ms: 0,
        found: 0,
    });
    Ok(scan_id)
}

/// Ends a scan.
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
    let running = {
        let scanning = state.scanning.lock();
        scanning.as_ref().map(|running| running.scan_id.clone())
    };
    let running = running.ok_or_else(|| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(serde_json::json!({"scanId": scan_id}))
    })?;
    if running != scan_id {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "scanId": scan_id,
                "running": running,
            })),
        );
    }
    *state.scanning.lock() = None;
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
    // Taken out of the registry first. The other order leaves a storage failure that has already dropped the device from
    // memory while the database still calls it paired — and the next start brings it back with no explanation.
    state.host.lock().went_offline(parsed, state.started_at_ms);
    state.db().unpair_device(device_id).map_err(internal)?;
    Ok(())
}

/// A card for a device this kernel has paired with.
///
/// # Errors
///
/// [`IpcError::Internal`] when the stored identifier does not parse. See [`devices_snapshot`]: a corrupted row must not
/// become a missing card.
fn card_from_paired(state: &AppState, row: &synra_store::DeviceRow) -> Result<DeviceCard, IpcError> {
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
        online: state.host.lock().sessions().live_for(device_id).is_some(),
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
