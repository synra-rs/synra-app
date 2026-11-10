use std::sync::Arc;

use crate::state::AppState;

use super::super::helpers::{asked_intent, record_and_announce};

pub(crate) fn kernel_callback(
    kernel_state: Arc<AppState>,
) -> Arc<synra_core::listener::KernelEventCallback> {
    let plugin_events = kernel_state
        .plugins
        .as_ref()
        .and_then(|plugins| plugins.lock().event_service());
    let on_kernel_events: Arc<synra_core::listener::KernelEventCallback> = Arc::new(
        move |envelope: &synra_protocol::envelope::Envelope,
              peer: &synra_protocol::ids::DeviceId| {
            use synra_core::listener::KernelEventOutcome;
            use synra_protocol::event::events;
            let board = Arc::clone(&kernel_state.board);
            let now = synra_store::sqlite::now_ms();
            let local = kernel_state.identity.lock().device_id;
            if envelope.event.is_plugin() {
                eprintln!(
                    "synra: plugin event received from={peer} event={} request={}",
                    envelope.event, envelope.request_id
                );
                return match synra_core::plugin_events::answer_remote_event(
                    plugin_events.as_ref(),
                    envelope,
                    peer,
                    local,
                    now,
                ) {
                    Ok(receipt) => {
                        eprintln!(
                            "synra: plugin event admitted from={peer} event={} request={}",
                            envelope.event, envelope.request_id
                        );
                        KernelEventOutcome::Receipt(Box::new(receipt))
                    }
                    Err(error) => {
                        eprintln!(
                            "synra: plugin event refused from={peer} event={} request={} error={error}",
                            envelope.event, envelope.request_id
                        );
                        KernelEventOutcome::Refused(error.to_string())
                    }
                };
            }
            match envelope.event.as_str() {
                events::INTENT_INVOKE => {
                    // ★ The engine is looked up **inside** this arm: a late result and a withdrawal only need the
                    // board, and refusing them because this window cannot run plugins would drop an outcome for an
                    // action the peer already performed.
                    let Some(plugins) = kernel_state.plugins.as_ref().map(Arc::clone) else {
                        return KernelEventOutcome::Refused(
                            "this window has no plugin engine, so it cannot run the action"
                                .to_owned(),
                        );
                    };
                    match synra_core::dispatch::orchestrate::accept_remote(
                        &plugins, &board, local, envelope, now,
                    ) {
                        Ok(Some(reply)) => {
                            // ★ The intent a peer asked this window to run has to reach the board's readers and
                            // the disk. Without this the action happened and the home page never showed it — and a
                            // restart forgot the peer's request entirely, so its retry ran the action again.
                            if let Some(id) = asked_intent(&kernel_state, envelope) {
                                record_and_announce(&kernel_state, id);
                            }
                            KernelEventOutcome::Reply(Box::new(reply))
                        }
                        // The same request twice with the first copy still running: the first copy answers.
                        Ok(None) => KernelEventOutcome::Handled,
                        Err(error) => KernelEventOutcome::Refused(error.to_string()),
                    }
                }
                events::INTENT_CANCEL => {
                    match synra_core::dispatch::orchestrate::accept_cancel(&board, envelope) {
                        Ok(Some(id)) => {
                            record_and_announce(&kernel_state, id);
                            KernelEventOutcome::Handled
                        }
                        Ok(None) => KernelEventOutcome::Handled,
                        Err(error) => KernelEventOutcome::Refused(error.to_string()),
                    }
                }
                // The late answer to an action this window dispatched: the receiver waited longer than this side
                // did and dialled back. `apply_result` ignores one for work no longer in flight.
                events::INTENT_RESULT => {
                    match synra_core::dispatch::orchestrate::apply_result(&board, envelope) {
                        Ok(Some(id)) => {
                            record_and_announce(&kernel_state, id);
                            KernelEventOutcome::Handled
                        }
                        Ok(None) => KernelEventOutcome::Handled,
                        Err(error) => KernelEventOutcome::Refused(error.to_string()),
                    }
                }
                _ => KernelEventOutcome::Ignored,
            }
        },
    );

    on_kernel_events
}
