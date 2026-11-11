use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::Mutex;

use synra_core::intent::Board;
use synra_core::{Host, Identity, shared};
use synra_ipc::IpcError;
use synra_store::{Paths, SynraDb};

use super::{AppState, internal, metrics::observe_db_writes};

impl AppState {
    /// A state over an already-open database, with no plugin host.
    ///
    /// # Errors
    ///
    /// [`IpcError`] when the identity cannot be read or written. A shell with no identity has nothing to show a user
    /// and nothing to present to a peer, so this is a start-up refusal rather than a degraded mode.
    pub fn new(
        mut db: SynraDb,
        paths: Paths,
        platform: &str,
        now_ms: i64,
    ) -> Result<Self, IpcError> {
        let started = std::time::Instant::now();
        let identity = Identity::load(&db, platform, now_ms).map_err(internal)?;
        // Written back because `load` generates one on a fresh installation, and a generated identity that is only in
        // memory is a device that appears new to every peer on every start — which loses every pairing, silently.
        identity.store(&db, now_ms).map_err(internal)?;
        // ★ A previous run's uninstall may have left an artifact directory behind: a loaded engine held its wasm file
        // open, the row was already gone, and the user was told. This is the moment to finish it — the old process is
        // gone and no plugin has been loaded. The sandbox is not swept: data kept on purpose belongs to the user.
        if let Ok(rows) = db.plugins() {
            let installed: Vec<String> = rows.into_iter().map(|row| row.plugin_id).collect();
            match synra_store::PluginFiles::new(paths.clone()).sweep_orphans(&installed) {
                Ok(sweep) if !sweep.removed.is_empty() => eprintln!(
                    "synra: removed {} leftover plugin director{} ({} bytes)",
                    sweep.removed.len(),
                    if sweep.removed.len() == 1 { "y" } else { "ies" },
                    sweep.bytes_freed
                ),
                Ok(sweep) => {
                    for path in sweep.leftovers {
                        eprintln!("synra: could not remove {}", path.display());
                    }
                }
                Err(error) => eprintln!("synra: the plugin directory could not be swept: {error}"),
            }
        }
        let peer = synra_core::LocalPeer::new(
            identity.device_id,
            identity.display_name.clone(),
            synra_core::platform_of_os(platform),
            env!("CARGO_PKG_VERSION"),
        );

        // ★ Built before the host, because the host is given one.
        //
        // `Host::with_metrics` existed for the daemon and the window built its host without one, so `frames_sent_total`,
        // `frames_received_total` and `links_opened_total` were written by nothing in the one host a desktop user runs.
        let metrics = Arc::new(synra_observe::metrics::RingRecorder::new());
        // ★ The store reports every write's duration to this recorder. Installed once, where the database is owned, rather
        // than at each call site — the store's write methods are the one funnel every write goes through.
        observe_db_writes(&mut db, &metrics);

        // ★ The board survives a restart, and it did not before: a window that closed forgot every intent — including a
        // remote action a peer was still waiting on, so the peer retried and the action ran twice. The mapping is the kernel's
        // (`synra_core::intent::persist`), the same one the daemon writes; this only decides what a failure means.
        let mut board = Board::new();
        match synra_core::intent::persist::restore(&mut board, &db) {
            Ok(report) => {
                if report.restored > 0 {
                    eprintln!(
                        "synra: restored {} intent(s) from the last run",
                        report.restored
                    );
                }
                for (intent, why) in report.unreadable {
                    eprintln!(
                        "synra: stored intent {intent} could not be read back and was skipped: {why}"
                    );
                }
                for intent in report.duplicates {
                    eprintln!("synra: intent {intent} was restored twice");
                }
            }
            // A host that refused to start over unreadable rows would turn one bad document into the loss of every intent
            // behind it — including the ones a peer is waiting on. The absence is named rather than silent.
            Err(error) => eprintln!("synra: the stored intents could not be read: {error}"),
        }

        // ★ The process facts are kept between reads rather than rebuilt: `sysinfo` computes CPU usage as a delta between
        // two refreshes, so a fresh `System` would report zero on every panel poll. The first refresh here is the baseline.
        let process_pid = sysinfo::Pid::from_u32(std::process::id());
        let mut process_system = sysinfo::System::new();
        let _ = process_system
            .refresh_processes(sysinfo::ProcessesToUpdate::Some(&[process_pid]), true);

        let host = shared(Host::new(identity.clone(), peer).with_metrics(Arc::clone(&metrics)));
        let db = Arc::new(Mutex::new(db));
        let events = Arc::new(synra_core::bus::EventBus::new());
        let input = crate::input::Input::new(Arc::clone(&db), &host, Arc::clone(&events));
        let state = Self {
            process_pid,
            process_system: Mutex::new(process_system),
            host,
            input,
            db,
            paths,
            identity: Arc::new(Mutex::new(identity)),
            board: Arc::new(Mutex::new(board)),
            intent_retries: Arc::new(Mutex::new(std::collections::HashMap::new())),
            plugins: None,
            plugin_loads: Mutex::new(synra_core::plugin::LoadedSummary::default()),
            engine_refusal: None,
            discovery: None,
            scanning: Arc::new(Mutex::new(None)),
            scans_started: AtomicU64::new(0),
            bundles_written: AtomicU64::new(0),
            listener: None,
            transfer_desk: Arc::new(crate::transfer_desk::TransferDesk::new()),
            uploads: crate::commands::plugin_ui::Uploads::new(),
            started_at_ms: now_ms,
            started: std::time::Instant::now(),
            uptime_reported: Mutex::new(0),
            // Plugin grants are the single user-facing gate for host effects. The desktop platform itself should not
            // ask for a second hidden simulate-keys switch after the plugin permission has been approved.
            platform: Arc::new(
                synra_platform::CommandPlatform::new()
                    .with_policies(synra_platform::Policies::new().with_simulate_keys(true)),
            ),
            metrics,
            log: None,
            events,
            ui_sessions: Arc::new(synra_plugin_ui::UiSessionRegistry::new()),
            hosted_ui: Mutex::new(std::collections::HashMap::new()),
            transfers: crate::commands::transfers::Transfers::new(),
            inbound_cancels: Arc::new(Mutex::new(std::collections::HashSet::new())),
            registry_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        // ★ One prune per construction, not per request: a cache whose files are never removed is a directory that grows with
        // every registry this device ever asked about. A failure to prune is a diagnostic, not a reason to refuse to start.
        crate::commands::registry::prune_documents(&state, now_ms);

        // ★ And the audit table gets the same treatment for the same reason: the sink writes a row on every gated call, and
        // nothing in this shell swept them. The retention window is configured (`audit.retentionDays`), defaulting to the
        // design's seven days; see `prune_audit`.
        crate::commands::plugins::prune_audit(&state, now_ms);
        // ★ The intents table gets the same treatment, and for the same reason: a row is written on every change, so a shell
        // that restores without pruning grows a history it never reads. The window is the design's "24 hours after they
        // finish" (`17 §6`), and it is one constant in the kernel because the daemon sweeps the same table.
        //
        // ★ Before the sweep, the other half of a restored board: a `Dispatching` intent is work whose answer is still coming —
        // but only for as long as the far side's late queue keeps it. Once that window has passed no answer can arrive, and a
        // row that says "running" for ever is both wrong on the home page and uncollectable (the sweep only takes finished
        // intents). The kernel decides; this writes the rows so the decision survives the next start too.
        let expired = {
            let mut board = state.board.lock();
            synra_core::intent::persist::expire_unanswered(&mut board, now_ms)
        };
        for intent in &expired {
            crate::commands::intent::checkpoint(&state, *intent);
        }
        if !expired.is_empty() {
            eprintln!(
                "synra: {} dispatched intent(s) ended without an answer",
                expired.len()
            );
        }
        match synra_core::intent::persist::collect(
            &state.db(),
            synra_core::intent::persist::INTENT_RETENTION_MS,
            now_ms,
        ) {
            Ok(0) => {}
            Ok(removed) => {
                eprintln!("synra: removed {removed} finished intent(s) past their retention")
            }
            Err(error) => eprintln!("synra: finished intents could not be collected: {error}"),
        }
        // ★ What it cost to get here — the shell's own half of M4's cold-start budget. Recorded on the recorder the
        // performance panel reads, so "cold start < 500 ms" can be checked against a number rather than a stopwatch.
        state.metrics.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "startup_duration_ms",
                [("phase".to_owned(), "state".to_owned())],
            ),
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        Ok(state)
    }
}
