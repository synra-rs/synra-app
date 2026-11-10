use super::*;

/// The body both entries share.
///
/// `started` is when the attempt began — before the dial when there is one, so `transfer_duration_ms` covers the whole
/// attempt rather than only the block loop. A caller that already has a link passes `Instant::now()`.
pub(crate) fn send_ready_since(
    state: &crate::state::AppState,
    to: synra_protocol::ids::DeviceId,
    link: &mut synra_core::link::PeerLink,
    mut sender: synra_core::transfer::TransferSender,
    mut row: TransferSnapshot,
    cancel: Option<&synra_core::supervisor::Cancellation>,
    started: std::time::Instant,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    use synra_protocol::ErrorCode;

    let transfer_id = row.transfer_id;
    announce_transfer(state, row.clone());

    row.state = "active".to_owned();
    announce_transfer(state, row.clone());

    let cancelled = || cancel.is_some_and(|token| token.is_cancelled());
    let deadline = started + synra_protocol::session::SessionTimeouts::default().transfer_total;
    let options = synra_core::SendOptions::new(
        synra_protocol::ids::TraceId::new(),
        unix_millis() as i64,
        deadline,
    );
    let outcome =
        synra_core::send_prepared(link, &mut sender, options, cancelled, |done, _total| {
            if !cancelled() {
                row.blocks_done = done;
                announce_transfer(state, row.clone());
            }
        });
    let ended = match &outcome {
        Ok(_) => SendOutcome::Delivered,
        Err(synra_core::SessionError::Cancelled) => SendOutcome::Cancelled,
        Err(_) => SendOutcome::Failed,
    };
    record_send(state, &row.kind, row.total_bytes, started.elapsed(), ended);

    finish_transfer(state, transfer_id);

    outcome.map(|_| transfer_id).map_err(|error| {
        IpcError::unknown(ErrorCode::TransferFailed).with_params(
            serde_json::json!({"deviceId": to.to_string(), "detail": error.to_string()}),
        )
    })
}

/// Why an outbound transfer ended, as the metric label spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendOutcome {
    /// The peer confirmed the proposed object's digest.
    Delivered,
    /// The user stopped it before the loop finished.
    Cancelled,
    /// The link refused a block, or the loop failed.
    Failed,
}

impl SendOutcome {
    /// The label `transfers_total{outcome}` carries.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Records one outbound attempt: its result, how long it took, and — only when it delivered — how large it was.
///
/// # Why `transfer_bytes` only on delivery
///
/// The histogram answers "how large are the files this device sends". A cancelled or failed transfer never moved the
/// whole file, so recording the planned size would describe bytes that never went. The attempt itself is in
/// `transfers_total{kind, outcome}` and the time it cost is in `transfer_duration_ms`.
pub(crate) fn record_send(
    state: &crate::state::AppState,
    kind: &str,
    total_bytes: u64,
    elapsed: std::time::Duration,
    outcome: SendOutcome,
) {
    record_attempt(
        state,
        kind,
        total_bytes,
        Some(elapsed),
        outcome.as_str(),
        outcome == SendOutcome::Delivered,
    );
}
