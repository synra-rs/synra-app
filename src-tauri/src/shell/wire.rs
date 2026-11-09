use std::sync::Arc;

use crate::state::AppState;

/// # Why the sink lives in this process rather than the daemon
///
/// The window is the process that has a console to show it in. A daemon runs headless, and a sink there would collect
/// lines nobody reads. The tap itself is in `synra-observe`, so a link built by either host reports to the same place.
///
/// # Why installing always is acceptable
///
/// The sink pushes onto the already-bounded event bus; it does no formatting and cannot block a write. The console's
/// drawer can therefore be opened at any moment and still show what happened while it was closed — which is the point of
/// a debug view. A sink that only existed while the drawer was open would miss the failure the user opened it to see.
pub(super) fn with_wire_trace(state: AppState) -> AppState {
    let events = Arc::clone(&state.events);
    // ★ A transfer is thousands of blocks, and each is a frame. Reporting every one would fill the kernel event bus with
    // debug lines and could cost a device card or a pairing prompt its delivery — the console must observe the bus, never
    // compete with it. One block every 200 ms still shows a transfer moving; envelopes are never throttled.
    let last_block =
        std::sync::Mutex::new(std::time::Instant::now() - std::time::Duration::from_millis(200));
    synra_observe::wire::install(Arc::new(move |line| {
        if line.label.starts_with("chunk") {
            let mut last = last_block
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if last.elapsed() < std::time::Duration::from_millis(200) {
                return;
            }
            *last = std::time::Instant::now();
        }
        events.emit(synra_ipc::event::KernelEvent::DebugWire(Box::new(
            synra_ipc::event::DebugWire {
                at_ms: line.at_ms,
                direction: line.direction.as_str().to_owned(),
                peer: line.peer.clone(),
                label: line.label.clone(),
                bytes: u64::try_from(line.bytes).unwrap_or(u64::MAX),
            },
        )));
    }));
    state
}
