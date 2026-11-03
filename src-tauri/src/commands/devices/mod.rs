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
//! `devices_connect` and `devices_disconnect` do not exist, here or in the contract. A link is opened when there is
//! something to carry over it — a transfer dials, announces itself and is recorded. A bare "connect" would be a dial
//! with no payload: a session that exists only so a screen can say it exists.

use synra_ipc::{DeviceCard, DevicesSnapshot, IpcError};
use synra_protocol::DeviceId;

use crate::state::{AppState, internal};

mod cards;
mod scanning;
#[cfg(feature = "tauri-shell")]
pub(crate) use cards::card_for;
use cards::{card_from_paired, card_from_seen};
#[cfg(test)]
use scanning::{announce_cards, scan_budget, scan_with};
pub use scanning::{devices_scan_cancel, devices_scan_start};

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
        let card = card_from_seen(&state.host, row)?;
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

#[cfg(test)]
mod tests;
