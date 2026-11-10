use std::sync::Arc;

use crate::state::AppState;

pub(crate) fn offer_callback(
    offer_state: Arc<AppState>,
) -> Arc<synra_core::inbound::OfferCallback> {
    let offer_desk = Arc::clone(&offer_state.transfer_desk);
    let offer: Arc<synra_core::inbound::OfferCallback> =
        Arc::new(move |incoming: &synra_core::inbound::Offer<'_>| {
            use synra_core::inbound::OfferDecision;
            if !matches!(
                incoming.request.kind,
                synra_protocol::session::TransferKind::Attachment
            ) {
                return OfferDecision::Accept;
            }
            // ★ A sender the user chose to keep accepting is not asked about again. The check is before anything
            // is announced, so a remembered device does not flash a prompt on its way past.
            if crate::commands::transfers::auto_accept_peer(
                &offer_state,
                &incoming.peer.to_string(),
            ) {
                return OfferDecision::Accept;
            }
            let now = synra_store::sqlite::now_ms();
            let peer_name = offer_state
                .db()
                .paired_devices()
                .ok()
                .and_then(|devices| {
                    devices
                        .into_iter()
                        .find(|device| device.device_id == incoming.peer.to_string())
                })
                .map_or_else(|| incoming.peer.to_string(), |device| device.display_name);
            let pending = crate::transfer_desk::Pending {
                transfer_id: incoming.request.transfer_id.to_string(),
                peer: incoming.peer.to_string(),
                file_name: incoming.file_name.to_owned(),
                total_bytes: incoming.request.byte_length,
                started_at_ms: now,
            };
            let announced = crate::commands::transfers::offered_snapshot(incoming, &peer_name, now);
            let show = || crate::commands::transfers::announce_offer(&offer_state, announced);
            let accepted = offer_desk.ask(&pending, show, crate::transfer_desk::OFFER_TIMEOUT);
            if accepted.unwrap_or(false) {
                OfferDecision::Accept
            } else {
                OfferDecision::Refuse
            }
        });

    offer
}
