use std::sync::Arc;

use super::Started;
use crate::state::AppState;
use parking_lot::Mutex;

pub(crate) fn progress_callback(
    progress_state: Arc<AppState>,
    progress_started: Started,
) -> Arc<synra_core::listener::ProgressCallback> {
    let names: Arc<Mutex<std::collections::HashMap<String, String>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let progress: Arc<synra_core::listener::ProgressCallback> = Arc::new(move |event| {
        // ★ A sender's abort is the last event of a transfer that produced no file. The row is removed rather than
        // marked completed: nothing arrived, and a "completed" row for a file that does not exist is a lie the
        // panel would keep until the next restart.
        if event.aborted {
            // ★ Counted before the row goes: "someone sent and it did not land" is a transfer, and the size is
            // deliberately not recorded — this histogram describes files that arrived.
            let started = progress_started.lock().remove(&event.progress.transfer_id);
            crate::commands::transfers::record_received(
                &progress_state,
                if event.progress.bundle.is_some() {
                    "plugin-bundle"
                } else {
                    "attachment"
                },
                event.progress.received_bytes,
                started.map(|started| started.elapsed()),
                true,
            );
            crate::commands::transfers::withdraw_received(
                &progress_state,
                event.progress.transfer_id,
            );
            return;
        }
        // The first event for a transfer is its start; later blocks leave the time alone.
        progress_started
            .lock()
            .entry(event.progress.transfer_id)
            .or_insert_with(std::time::Instant::now);
        let key = event.peer.to_string();
        let name = {
            let mut names = names.lock();
            if let Some(name) = names.get(&key) {
                name.clone()
            } else {
                let looked_up = progress_state
                    .db()
                    .paired_devices()
                    .ok()
                    .and_then(|devices| devices.into_iter().find(|device| device.device_id == key))
                    .map_or_else(|| key.clone(), |device| device.display_name);
                names.insert(key.clone(), looked_up.clone());
                looked_up
            }
        };
        let snapshot = crate::commands::transfers::received_snapshot(
            event,
            &name,
            synra_store::sqlite::now_ms(),
        );
        crate::commands::transfers::announce_transfer(&progress_state, snapshot);
    });

    progress
}
