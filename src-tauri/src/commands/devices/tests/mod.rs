//! Tests for the devices commands.
//!
//! # Paired, and heard from but not paired
//!
//! Both appear in a snapshot and both are exercised here, because the kernel writes `devices_seen` (the daemon's
//! discovery thread) as well as `paired_devices` (the handshake). An earlier version of this file could not cover the
//! second set at all — the table existed, the reader existed, and nothing wrote it — which is what a reader without a
//! writer looks like from the outside.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use synra_ipc::{ScanOptions, ScanState};
use synra_protocol::ErrorCode;
use synra_store::{Paired, Seen};

use super::{
    devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot, scan_budget,
};
use crate::state::AppState;
use crate::testing;

const OTHER: &str = "22222222-2222-4222-8222-222222222333";

/// A state with one paired device in it.
fn with_paired_device() -> AppState {
    let state = testing::state();
    let identity = state.identity_snapshot(crate::KERNEL_VERSION);
    state
        .db()
        .pair_device(&Paired {
            device_id: identity.device_id.to_string(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            address: Some("192.168.1.4:5000".to_owned()),
        })
        .expect("pairs a device");
    state
}

/// A sighting of a device that has never been paired, as discovery would record it.
fn sighting(id: &str, name: &str) -> Seen {
    Seen {
        device_id: id.to_owned(),
        display_name: name.to_owned(),
        platform: "linux".to_owned(),
        seen_at_ms: 3_000,
        source: "192.168.1.9:5000".to_owned(),
    }
}

/// A state whose shell bound the discovery sockets, on ports the operating system chose.
///
/// ★ Port zero rather than the well-known discovery port: binding that would collide with a kernel that happens to be
/// running on this machine, and a test that needed it free would fail for a reason unrelated to what it asserts. The
/// deadline is handed back because a scan publishes one, waits, and takes it down again — so watching it is the only way
/// to see that the worker was asked at all.
fn with_discovery() -> (AppState, Arc<AtomicI64>) {
    let state = testing::state();
    let discovery = crate::discovery::Discovery::start(
        state.announcement(1),
        0,
        Arc::clone(&state.db),
        state.host.clone(),
        0,
        Arc::new(|_: &str| {}),
    )
    .expect("the sockets are free");
    let deadline = discovery.on_demand();
    (state.with_discovery(discovery), deadline)
}

/// An immediate read, which does not disturb an admitted network scan.
fn cached() -> ScanOptions {
    ScanOptions {
        cached_only: true,
        ..ScanOptions::default()
    }
}

/// An admitted task for argument-validation tests; lifecycle tests use the real discovery path below.
fn scanning(state: &AppState, scan_id: &str) {
    *state.scanning.lock() = Some(crate::state::ScanTask::new(ScanState {
        scan_id: scan_id.to_owned(),
        elapsed_ms: 0,
        total_ms: 5_000,
        found: 0,
    }));
}

mod announcements;
mod forget;
mod scan_admission;
mod scan_progress;
mod snapshots;
