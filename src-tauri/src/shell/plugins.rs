use std::sync::Arc;

use crate::commands;
use crate::state::AppState;

/// The plugin set when an engine starts, and a shell without one when it does not.
///
/// # Errors
///
/// None. This is the one failure in start-up that does not stop the shell, and that is the whole point of the function.
///
/// ★ An engine that will not start — a missing system library, a CPU feature this build was not compiled for — is a shell
/// that still shows devices, settings and diagnostics. Dropping the window over it would turn a recoverable condition
/// into an unusable application, and the plugin half is the smaller loss.
///
/// ★ This is also what keeps an Android build a *shell* rather than a black screen when the engine cannot start: the
/// `wasmtime` engine is the largest and most platform-sensitive dependency in the tree, and this function is the seam
/// that lets the rest of the application survive it.
pub(super) fn with_plugins_or_refuse(
    state: AppState,
    verdict: &synra_observe::crash::StartupVerdict,
) -> AppState {
    // ★ Before the engine, because it is about what is on disk rather than what can run. A directory an author copied
    // into this shell's data directory becomes a row here — disabled and untrusted, so the consent screen is still the
    // gate — and the list can show it even in a build with no engine at all.
    let _ = commands::plugins::discover_plugins(&state, state.started_at_ms);
    // ★ The engine the platform wants, asked for rather than assumed.
    //
    // `BackendKind::default_for` is the table the kernel keeps for exactly this question, and a shell that named
    // `WasmtimeCranelift` itself would be a second answer to it — one that is wrong on a phone, where a JIT costs build
    // time and a baseline compiler is the compromise the design document names.
    let host_kind = if cfg!(any(target_os = "android", target_os = "ios")) {
        synra_plugin_host::HostKind::Mobile
    } else {
        synra_plugin_host::HostKind::Desktop
    };
    let kind = synra_plugin_host::BackendKind::default_for(host_kind);
    match synra_plugin_host::engine::WasmtimeBackend::new(kind) {
        Ok(engine) => {
            // ★ The host, which this shell had never supplied.
            //
            // `PluginSet::new` took an engine alone when this line was written and takes a host as well now. The second
            // argument is not a detail: an instance with no session refuses every host call, which is what made the
            // capability gate inert, and requiring the host is what made that state unrepresentable.
            let host = Arc::new(
                crate::plugin_host::DesktopHost::with_platform(
                    Arc::clone(&state.db),
                    Arc::clone(&state.platform),
                )
                .with_network(&state.host)
                .with_input_controller(Arc::clone(&state.input.controller)),
            );
            let event_delivery = host.events();
            let backend = synra_plugin_host::Backend::new(Arc::new(engine));
            // ★ The same question the daemon asks, for the same reason: an engine that cannot enforce what this
            // host will ask of it must not run third-party code at all.
            //
            // ★ And here the answer costs the **plugin half only**, which is the shape this function already has: the window
            // still shows devices, settings and diagnostics. An engine missing a limit joins an engine that would not start,
            // rather than becoming an engine that runs plugins without the bound.
            match synra_plugin_host::engine::check_enforceable(
                &backend,
                &synra_plugin_host::quota::Quota::default(),
            ) {
                Ok(()) => {
                    // ★ And the audit gets its durable half, which the shell never had.
                    //
                    // The daemon has installed this sink since the table's writer was written; the shell did not, so a
                    // plugin's audit trail here lived in the in-memory ring and **died with the process** — while the
                    // migration that created `plugin_audit` says what the table is for: *"the durable part is what makes the
                    // interface's 'recent activity' survive a restart."*
                    let set = synra_core::dispatch::PluginSet::new(backend, host)
                        .with_event_delivery(event_delivery)
                        .with_audit_sink(crate::plugin_host::audit_sink(Arc::clone(&state.db)))
                        // ★ And the same recorder the `diag_metrics` command reads, so the counts a plugin's
                        // capability requests produce are reachable from the window rather than only from a log line.
                        .with_metrics(Arc::clone(&state.metrics))
                        // ★ The health rung the card draws. `PluginHealthChanged` was in the contract and had no
                        // publisher; the sink is the set's way of saying "this plugin moved" without depending on the
                        // interface's event vocabulary.
                        .with_health_sink(Arc::new({
                            let events = Arc::clone(&state.events);
                            move |id: &synra_protocol::ids::PluginId,
                                  health: &str,
                                  explanation: &str| {
                                events.emit(synra_ipc::event::KernelEvent::PluginHealthChanged {
                                    plugin_id: id.clone(),
                                    health: health.to_owned(),
                                    explanation: explanation.to_owned(),
                                });
                            }
                        }));
                    // ★ The set is built, and then it is **filled**. Until this line the shell created an empty set and
                    // never called the loader, so every plugin was listed, could be enabled, and was never askable —
                    // `intent_dispatch` matched nothing and `life_state` called it `unavailable` forever.
                    let shared = match set.into_shared() {
                        Ok(shared) => shared,
                        Err(error) => return state.with_engine_refusal(error.to_string()),
                    };
                    let state = state.with_plugins(shared);
                    let revocations = commands::plugins::revocations(&state);
                    let summary =
                        commands::plugins::load_installed_plugins(&state, verdict, &revocations);
                    state.with_plugin_loads(summary)
                }
                Err(error) => {
                    // The same shape as the arm below: said once, and the shell carries on without the plugin half.
                    eprintln!(
                        "synra: plugins will not run in this build: this engine cannot enforce the limits this shell sets: {error}"
                    );
                    state.with_engine_refusal(format!(
                        "this build cannot enforce the limits a plugin needs: {error}"
                    ))
                }
            }
        }
        Err(error) => {
            // ★ Said once, here, and then carried by every command that needs a plugin.
            //
            // `AppState::plugin_set` is where the user-facing refusal is produced, with the reason attached. Saying it
            // again per command would be two statements of one fact, and the second is the one nobody reads.
            eprintln!("synra: plugins will not run in this build: {error}");
            state.with_engine_refusal(format!("this build has no plugin engine: {error}"))
        }
    }
}
