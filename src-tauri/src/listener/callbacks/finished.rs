use std::sync::Arc;

use super::Started;
use crate::state::AppState;

use super::super::helpers::{notifications_enabled, received_notice, received_notification};

pub(crate) fn finished_callback(
    finished_state: Arc<AppState>,
    finished_started: Started,
) -> synra_core::listener::FinishedCallback {
    let finished: synra_core::listener::FinishedCallback = Arc::new(
        move |transfer: &synra_core::listener::FinishedTransfer| {
            // ★ The bytes are a `.synra` package now, so receiving one is **installing** it: the same pipeline the
            // install command runs, over the archive the peer sent. `record_received` then writes the history and
            // the sync state — and its plugin-row insert fails harmlessly, because the installer already made one
            // with a version attached.
            let (outcome, reconcile) = {
                let mut db = finished_state.db();
                match &transfer.bundle {
                    Some(plugin_id) => {
                        match crate::commands::plugins::install_package(
                            &finished_state,
                            &mut db,
                            &transfer.path,
                        ) {
                            Ok(installed) => (
                                synra_core::plugin_sync::record_received(&db, transfer),
                                Some(installed.plugin_id),
                            ),
                            Err(error) => (
                                Some(synra_core::plugin_sync::SyncOutcome::refused(
                                    plugin_id.clone(),
                                    synra_core::plugin_sync::SyncSessionId::new(
                                        transfer
                                            .sync_session_id
                                            .map_or_else(String::new, |session| {
                                                session.to_string()
                                            }),
                                    ),
                                    error,
                                )),
                                None,
                            ),
                        }
                    }
                    None => (
                        synra_core::plugin_sync::record_received(&db, transfer),
                        None,
                    ),
                }
            };
            if let Some(plugin_id) = reconcile
                && let Err(error) =
                    crate::commands::plugins::reconcile_after_install(&finished_state, &plugin_id)
            {
                eprintln!(
                    "synra: synced plugin {plugin_id} was installed but could not refresh its runtime: {error}"
                );
            }
            // ★ And the row this side opened when the proposal arrived. The receiving half of a transfer has no
            // worker to remove it; without this the panel would show a transfer as arriving for ever.
            if let Some(transfer_id) = transfer.transfer_id {
                crate::commands::transfers::complete_received(&finished_state, transfer_id);
            }
            // ★ The receiving half's measurement, beside the row it closes. The kind comes from the same fact
            // the install branch above used, so a bundle is counted as a bundle.
            let started = transfer
                .transfer_id
                .and_then(|transfer_id| finished_started.lock().remove(&transfer_id));
            crate::commands::transfers::record_received(
                &finished_state,
                if transfer.bundle.is_some() {
                    "plugin-bundle"
                } else {
                    "attachment"
                },
                transfer.byte_length,
                started.map(|started| started.elapsed()),
                false,
            );
            // ★ And the receiver gets told where it landed. The row disappears when the transfer finishes, so
            // without this the file simply stops being visible — the one person who needs to find it is the one
            // looking at the screen. A bundle is different: it was installed, and the plugins page says so.
            if let Some(notice) = received_notice(transfer) {
                finished_state.publish(notice);
            }
            // ★ The window may not be in front. Best effort: a platform that cannot show a notification is a
            // missing convenience, not a reason to fail a transfer that already landed.
            if let Some((title, body)) =
                received_notification(transfer, crate::locale::current(&finished_state))
                && notifications_enabled(&finished_state)
            {
                use synra_platform::Platform as _;
                let _ = finished_state.platform.notify(&title, &body);
            }
            outcome
        },
    );

    finished
}
