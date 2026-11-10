use std::sync::Arc;

use crate::state::AppState;

/// Binds the port this device is reachable on, and records it in the state.
///
/// # Why this is separate from [`spawn`]
///
/// Because discovery announces the port a peer should **dial**, and it starts before the window is managed. Binding first is what
/// lets the announcement carry the listener's port rather than the discovery responder's — the difference between a peer reaching
/// this device and a peer reaching a service that only answers datagrams.
///
/// # What a failed bind costs
///
/// The reachability, and nothing else. The window opens, the scan works, and `pairing_request` reports the failure of a dial rather
/// than the interface losing itself.
#[must_use]
pub fn bind(mut state: AppState) -> AppState {
    // Commands, incoming connections and the event sink must use the same pairing service.
    // A second Host has its own desk, so its request can never reach the UI or receive the user's answer.
    let host = Arc::clone(&state.host);

    let bound = {
        let mut guard = host.lock();
        synra_core::listener::bind(&mut guard, 0).map(|address| address.port())
    };

    match bound {
        Ok(port) => {
            state.listener = Some((host, port));
        }
        Err(error) => {
            eprintln!("synra: the listener could not bind a port: {error}");
        }
    }
    state
}
