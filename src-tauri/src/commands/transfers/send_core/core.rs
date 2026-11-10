use super::*;

/// What this shell says about itself when it dials a peer.
///
/// # Why the platform is mapped rather than passed through
///
/// Because the identity stores the platform as a string (what the installer recorded) while the wire wants `PlatformKind`,
/// and the two must not drift: a peer reading "windows" where it expected an enum would decline the handshake, and the shell
/// would look like a broken build rather than a mistyped one.
///
/// # Why the display name comes from the identity
///
/// Because that is the name the user set and the one the discovery responder already announces. A second source would let
/// one device appear under two names on the same network.
#[must_use]
pub fn local_peer(identity: &synra_core::identity::Identity) -> synra_core::peer::LocalPeer {
    use synra_protocol::session::PlatformKind;

    let platform = match identity.platform.to_ascii_lowercase().as_str() {
        "android" => PlatformKind::Android,
        "ios" | "macos" | "mac" => PlatformKind::Ios,
        _ => PlatformKind::Desktop,
    };
    synra_core::peer::LocalPeer::new(
        identity.device_id,
        identity.display_name.clone(),
        platform,
        env!("CARGO_PKG_VERSION"),
    )
}

/// Sends a file to a paired device, and answers the transfer it started.
///
/// # Where the address comes from
///
/// `paired_devices`, not `devices_seen`: a transfer's target must already be paired, and that table is the one that records
/// where a device is. `None` is not bad data — it means this device has never been dialled — so it becomes
/// [`ErrorCode::Unsupported`] with a reason a person can act on.
///
/// # Why the row is recorded before the dial
///
/// So a send that fails to connect leaves a row saying why instead of nothing at all. The row is removed on every exit path and
/// `TransferFinished` is announced on each: a transfer that stayed in the registry after failing would look like one still
/// running.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] for an unknown or address-less device, [`ErrorCode::InvalidArgument`] for a file that cannot be
/// planned, and [`ErrorCode::TransferFailed`] for a dial or a block the link refused.
pub fn transfer_send_file(
    state: &crate::state::AppState,
    request: &synra_ipc::dto::SendFileRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let address = address_of(state, request.to)?;

    let sender = synra_core::transfer::TransferSender::open(
        std::path::Path::new(&request.path),
        synra_core::transfer::DEFAULT_CHUNK_SIZE,
    )
    .map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": request.path, "reason": error.to_string()}))
    })?;
    let planned = sender.request();

    let now_ms = synra_core::host::millis_reading(unix_millis());
    let mut row = snapshot_from(
        &planned,
        (request.to, &request.to.to_string()),
        "queued",
        now_ms,
    );
    // The row names the file a person chose, not the path the interface happened to pass.
    row.file_name = std::path::Path::new(&request.path).file_name().map_or_else(
        || request.path.clone(),
        |name| name.to_string_lossy().into_owned(),
    );

    send_over(state, request.to, address, sender, row)
}

/// Dials, moves every block, and reports each one — the part two commands share.
///
/// # Why this is a function
///
/// Because `transfer_send_file` and `transfer_send_text` differ only in how they obtain a [`TransferSender`] and what the row
/// is named: everything after "there is a sender and an address" is the same dial, the same loop and the same three exit
/// paths. Two copies would be two places for the cleanup to be forgotten, and the cleanup is what keeps a failed transfer
/// from looking like a running one.
///
/// # Errors
///
/// [`ErrorCode::TransferFailed`] for a refused dial or a refused block, after removing the row either way.
pub fn send_over(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    address: std::net::SocketAddr,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_over_with(state, to, address, sender, row, None)
}

/// [`send_over`], with a token a cancel can stop.
pub fn send_over_with(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    address: std::net::SocketAddr,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let started = std::time::Instant::now();
    let kind = row.kind.clone();
    let total_bytes = row.total_bytes;
    let transfer_id = row.transfer_id;
    let now_ms = unix_millis();

    let parts = state.host.lock().dialling_parts();
    let dialled = match synra_core::host::dial_peer(&parts.0, parts.1, address, to, now_ms) {
        Ok(dialled) => dialled,
        Err(error) => {
            // ★ A dial that never connected is still an attempt, and the one a user most wants counted: it is the failure
            // that happens before any block moves. `send_ready_since` records the other exits.
            record_send(
                state,
                &kind,
                total_bytes,
                started.elapsed(),
                SendOutcome::Failed,
            );
            finish_transfer(state, transfer_id);
            return Err(IpcError::unknown(ErrorCode::TransferFailed).with_params(
                serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()}),
            ));
        }
    };
    // ★ The handshake is the only moment this side learns who the peer is, so it is where the pairing is written.
    // Nothing else records one: the identity layer that used to do it is gone (ADR-025), and a device list where
    // nothing is ever paired would offer "Forget" for rows that cannot exist.
    record_pairing(state, &dialled, address);

    let mut link = dialled
        .into_link()
        // ★ The byte counters live on the link; without this the shell would own a recorder nothing on its dials wrote to.
        .with_metrics(std::sync::Arc::clone(&state.metrics));
    send_ready_since(state, to, &mut link, sender, row, cancel, started)
}

/// Runs the complete transfer over an established link and reports acknowledged blocks.
///
/// # Why the link is a parameter
///
/// Protocol tests can provide an established link; production commands dial through the authenticated host.
pub fn send_ready(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_ready_with(state, to, link, sender, row, None)
}

/// [`send_ready`], cancellable during block transfer and the final confirmation wait.
pub fn send_ready_with(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    sender: synra_core::transfer::TransferSender,
    row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    send_ready_since(
        state,
        to,
        link,
        sender,
        row,
        cancel,
        std::time::Instant::now(),
    )
}
