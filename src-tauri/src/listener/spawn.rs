use std::sync::Arc;

use tauri::Manager as _;

use crate::state::AppState;

use super::callbacks;

/// Starts the listener and the late-answer retry loop.
#[must_use]
pub fn spawn(app: &tauri::AppHandle) -> Option<synra_core::supervisor::Cancellation> {
    let state = app.state::<Arc<AppState>>();
    let Some((host, port)) = state.listener.clone() else {
        eprintln!("synra: no port was bound, so this window cannot be reached");
        return None;
    };

    let cancel = synra_core::supervisor::Cancellation::new();
    let thread_cancel = cancel.clone();
    let data_dir = state.paths.data_dir().to_path_buf();
    {
        let host = Arc::clone(&host);
        let data_dir = data_dir.clone();
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            loop {
                let _ = synra_core::listener::retry_late(&host, &data_dir);
                if cancel.wait(std::time::Duration::from_secs(30)) {
                    break;
                }
            }
        });
    }

    let Some(plugins) = state.plugins.clone() else {
        eprintln!("synra: this window has no plugin host, so it does not listen");
        return None;
    };
    let router = Arc::new(synra_core::dispatch::orchestrate::Router::new(
        plugins,
        Arc::clone(&state.board),
    ));
    let db_state: Arc<AppState> = Arc::clone(state.inner());

    std::thread::Builder::new()
        .name(format!("synra-listener-{port}"))
        .spawn(move || {
            let inbound_started = callbacks::new_started();
            let finished =
                callbacks::finished_callback(Arc::clone(&db_state), Arc::clone(&inbound_started));
            let progress =
                callbacks::progress_callback(Arc::clone(&db_state), Arc::clone(&inbound_started));
            let offer = callbacks::offer_callback(Arc::clone(&db_state));
            let cancels = Arc::clone(&db_state.inbound_cancels);
            let should_cancel: Arc<synra_core::listener::CancelCallback> =
                Arc::new(move |id: &str| cancels.lock().remove(id));
            let on_kernel_events = callbacks::kernel_callback(Arc::clone(&db_state));

            synra_core::listener::serve_with(
                &host,
                &thread_cancel,
                &data_dir,
                &router,
                &finished,
                &progress,
                &offer,
                &should_cancel,
                &on_kernel_events,
            );
        })
        .map_or_else(
            |error| {
                eprintln!("synra: the listener thread could not start: {error}");
                None
            },
            |_| Some(cancel),
        )
}
