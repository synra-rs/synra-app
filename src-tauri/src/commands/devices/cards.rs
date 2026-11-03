#[cfg(feature = "tauri-shell")]
use crate::state::internal;
use synra_ipc::{DeviceCard, IpcError};
use synra_protocol::DeviceId;

/// A card for a device that was seen and never paired.
///
/// # Errors
///
/// [`IpcError::Internal`] when the stored identifier does not parse, for the reason [`devices_snapshot`] gives: a
/// corrupted row must not become a missing card.
pub(super) fn card_from_seen(
    host: &synra_core::SharedHost,
    row: &synra_store::SeenDeviceRow,
) -> Result<DeviceCard, IpcError> {
    let device_id = parse_from_store(row.device_id.as_str())?;
    Ok(DeviceCard {
        device_id,
        display_name: row.display_name.clone(),
        platform: row.platform.clone(),
        online: host
            .lock()
            .recent_address(device_id, synra_store::sqlite::now_ms())
            .is_some(),
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
        Some(row) => card_from_seen(host, row).map(Some),
        None => Ok(None),
    }
}

/// A card for a device this kernel has paired with.
///
/// # Errors
///
/// [`IpcError::Internal`] when the stored identifier does not parse. See [`devices_snapshot`]: a corrupted row must not
/// become a missing card.
pub(super) fn card_from_paired(
    host: &synra_core::SharedHost,
    row: &synra_store::DeviceRow,
) -> Result<DeviceCard, IpcError> {
    let device_id = parse_from_store(row.device_id.as_str())?;
    let presence = super::super::peers::presence(host, row, synra_store::sqlite::now_ms());
    Ok(DeviceCard {
        device_id,
        display_name: row.display_name.clone(),
        platform: row.platform.clone(),
        // Links are opened on demand; recent discovery also makes a paired device available.
        online: presence.online,
        paired: !row.blocked,
        blocked: row.blocked,
        last_seen_ms: presence.last_seen_ms,
        address: presence.address,
        transfer: None,
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
