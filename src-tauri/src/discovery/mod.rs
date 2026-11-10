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
//! # What it announces as its port
//!
//! ★ The port the shell's **transport listener** bound, because that is where a peer must dial. The listener binds an
//! ephemeral port (`listener::bind`), so it is read back from the state rather than assumed.
//!
//! This used to announce the discovery responder's port, because the window had no transport half yet. The result was a
//! device that could be found and never reached: every pairing dialled the UDP responder, and TCP cannot connect to it.
//!
//! [`DeviceAnnouncement::validate`]: synra_protocol::discovery::DeviceAnnouncement::validate

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use synra_core::discovery::Candidate;
use synra_core::supervisor::{Cancellation, Supervisor, TaskId, TaskSpec, run_task};
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

/// Releases only the demand window this call published, including during unwinding.
struct DemandWindow<'a> {
    deadline: &'a AtomicI64,
    value: i64,
}

impl Drop for DemandWindow<'_> {
    fn drop(&mut self) {
        let _ = self
            .deadline
            .compare_exchange(self.value, 0, Ordering::Relaxed, Ordering::Relaxed);
    }
}

mod service;

pub use service::Discovery;

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
