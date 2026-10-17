//! The shell's own listener: the half that makes this device reachable.
//!
//! # Why the shell listens at all
//!
//! Because it is a host in its own right — the same reason [`crate::discovery`] scans rather than asking the daemon. The two keep
//! **separate data directories**, so a connection the daemon accepts writes into the daemon's tables, and the window would go on
//! showing a device list that never learns what arrived. A desktop that needed a second program running beside it would be one
//! whose transfers only work while somebody starts that program.
//!
//! # What it is not yet
//!
//! `serve` is called with [`no_answer`](synra_core::listener::no_answer) as its finished callback, which means an accepted transfer
//! is recorded nowhere. The callback is where `sync_records` gets written, and until it is, `plugins_sync_plan` reports every peer as
//! holding nothing — see `design-docs/19 §8.7`.
//!
//! # Why it runs in its own thread
//!
//! Because `serve` loops until its cancellation is asked to stop: it must not own the thread that draws the window, and it must be
//! stoppable when the window closes, or a restart would fail to bind a port the old process still holds.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager as _;

use crate::state::AppState;

/// Starts the listener, and answers the token that stops it.
///
/// The token is returned rather than dropped so a caller can stop the listener on the way out. A thread whose token is thrown away
/// is one that can only be stopped by killing the process.
#[must_use]
pub fn spawn(app: &tauri::AppHandle) -> Option<synra_core::supervisor::Cancellation> {
    let state = app.state::<Arc<AppState>>();
    // Binding can fail for reasons a user can act on — another copy of this application already holding the port — and a failure
    // here must not stop the window from appearing. The command that needs a link reports its own refusal; this one only logs.
    let host = Arc::new(Mutex::new(synra_core::Host::new(
        state.identity.lock().clone(),
        local_of(&state),
    )));

    let port = match synra_core::listener::bind(&mut host.lock(), 0) {
        Ok(address) => address.port(),
        Err(error) => {
            eprintln!("synra: the listener could not bind a port: {error}");
            return None;
        }
    };

    let cancel = synra_core::supervisor::Cancellation::new();
    let thread_cancel = cancel.clone();
    let data_dir = state.paths.data_dir().to_path_buf();
    let desk = Arc::new(synra_core::pairing::PairingDesk::new(
        state.identity.lock().device_id,
        synra_protocol::session::SessionTimeouts::default().pairing,
    ));
    // ★ The router the kernel's listener wants is a thin pair of the Arcs this shell already holds — not the daemon's 660-line
    // orchestration. It needs a plugin set, and `PluginSet::new` refuses to exist without a host to route effects through, so a
    // window with no plugin host does not listen at all: one that accepted a share and could route it nowhere would be a link
    // that answers "yes" and then does nothing.
    let Some(plugins) = state.plugins.clone() else {
        eprintln!("synra: this window has no plugin host, so it does not listen");
        return None;
    };
    let router = Arc::new(synra_core::dispatch::orchestrate::Router::new(
        plugins,
        Arc::clone(&state.board),
    ));

    std::thread::Builder::new()
        .name(format!("synra-listener-{port}"))
        .spawn(move || {
            let finished = synra_core::listener::no_answer();
            synra_core::listener::serve(&host, &thread_cancel, &data_dir, &desk, &router, &finished);
        })
        .map_or_else(
            |error| {
                eprintln!("synra: the listener thread could not start: {error}");
                None
            },
            |_| Some(cancel),
        )
}

/// What this device says about itself on the wire.
fn local_of(state: &AppState) -> synra_core::peer::LocalPeer {
    let identity = state.identity.lock();
    synra_core::peer::LocalPeer::new(
        identity.device_id,
        identity.display_name.clone(),
        synra_protocol::session::PlatformKind::Desktop,
        env!("CARGO_PKG_VERSION"),
    )
}
