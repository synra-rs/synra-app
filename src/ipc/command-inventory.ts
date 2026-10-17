/**
 * The two lists `registration.test.ts` holds the shell to.
 *
 * # Why they are apart from the assertions
 *
 * The test is about **whether the shell agrees with the contract**, and these are the claim and the gap it is checked
 * against — data rather than assertions, and the bulk of the file. Reading them at one go is what makes the gap visible;
 * reading them between assertions is what made the test hard to follow.
 *
 * ★ `REGISTERED` is the claim, and it must stay hand-written: deriving it from the shell's `generate_handler!` would
 * make the test assert that a list equals itself. `NOT_YET_REGISTERED` is the gap, each entry with the reason it is
 * still there — a list of unbuilt things with no reason attached rots into a list nobody can act on.
 */

/** The commands `src-tauri/src/shell.rs` hands to `tauri::generate_handler!`. */
export const REGISTERED = [
  'identity_get',
  'identity_set_display_name',
  'devices_snapshot',
  'devices_scan_start',
  'devices_scan_cancel',
  'devices_forget',
  'plugins_list',
  'plugins_enable',
  'plugins_disable',
  'plugins_consent_respond',
  // ★ The install path, which moved from "no installer is wired" to registered when the pipeline became a kernel
  // function: `synra_core::install` is the same one the daemon runs.
  'plugins_install',
  // ★ Removable, with the data decision passed through rather than defaulted in two places.
  'plugins_uninstall',
  // ★ The audit trail: the ring had no writer until a plugin instance was given a session.
  'plugins_audit',
  // ★ The four the shell could not implement while the orchestration lived in the daemon binary.
  'intent_dispatch',
  // ★ And the fifth, which the interface had been pretending existed: `HomePage.vue`'s "received" panel called
  // `intent_snapshot` with no identifier and read `answer.value.intents`, so the kernel was asked a question it refuses
  // and the page reported a bridge failure. The board could list its intents all along; the contract had no name for it.
  'intent_list',
  'intent_snapshot',
  'intent_decide',
  'intent_cancel',
  'config_get',
  'config_set',
  'diag_export',
  'diag_logs_tail',
  // ★ And the metrics command, which came up the same way the log tail did: the gate has counted into a `RingRecorder`
  // since it learned to count, and what was missing here was a sink behind `AppState` rather than the command itself.
  // The reason it used to carry — "no metrics sink is behind AppState" — was true, and it was the *only* thing in the way.
  'diag_metrics'
] as const

/**
 * Declared by the IPC contract, not yet implemented by the shell.
 *
 * Each needs a handler wired to the kernel before it can move up. They are grouped by what they are waiting on, because
 * the wait is different in each group and a reader deserves to know which.
 */
export const NOT_YET_REGISTERED: Readonly<Record<string, string>> = {
  // ★ These three groups used to say the shell was one wiring job away, and it is not.
  //
  // Pairing said "needs the pairing desk wired to a Tauri command" — but there is **no desk in this shell at all**
  // (nothing here names `PairingDesk` outside a test name), and a desk with no link would have nothing to raise a prompt
  // over. Transfers said "the kernel sends, but no UI starts a transfer", which describes the *daemon*: this shell has no
  // transport, no `synra-transfer` or `synra-net` dependency, and its own plugin host answers `transfer-send` with
  // `notImplemented`. Sync moves bytes over links, so it needs the same missing half.
  //
  // What is missing is the shell's **network half** — the `DiscoveryService` and `TransportRegistry` that doc 05 §2.1 puts
  // inside the desktop app's in-process kernel. A `.vue` file is not the remaining work, and a reason that says it is
  // sends the next person to the wrong file.
  pairing_request: 'the shell has no pairing desk and no link to raise a prompt over',
  pairing_respond: 'the shell has no pairing desk and no link to answer on',
  pairing_cancel: 'the shell has no pairing desk and no link to cancel',
  // ★ The four intent commands are registered now, and the reason is worth keeping: they used to carry "needs the
  // intent board behind AppState", which was false — the board was already in the state. What was missing was the
  // orchestration, which lived in `synra-daemon/src/runtime/intent.rs` **inside a binary**. It is in
  // `synra_core::dispatch::orchestrate` now, and the shell's commands are the thin layer that turns DTOs into calls.
  // Transfers are implemented in the kernel but this shell has nothing to send them over.
  transfer_send_file: 'the shell has no transport to send one over',
  transfer_send_text: 'the shell has no transport to send one over',
  transfer_cancel: 'the shell has no transport, so nothing is in flight to cancel',
  transfer_list: 'the shell has no transport, so there is no transfer to list',
  // Connecting and disconnecting are deliberately absent — the kernel dials, the UI does not.
  //
  // ★ "The kernel dials" is still true of the daemon and not of this shell, which never dials anything. The reason stands
  // as an intent — the UI is not where a dial belongs — but it is not a claim that dialling happens here.
  devices_connect: 'deliberately absent: the kernel dials, the UI does not',
  devices_disconnect: 'deliberately absent: the kernel dials, the UI does not',
  // The plugin lifecycle beyond enable/disable/install is not built. `plugins_install` moved out of this list when the
  // pipeline became reachable from the shell: `synra_core::install` is the daemon's pipeline too, so the order of the
  // checks has one implementation and this shell supplies only what it can do (a desktop can open a window).
  plugins_catalog_search: 'no catalog client is wired',
  plugins_check_update: 'no update checker exists',
  plugins_update: 'no updater exists',
  // ★ These two are also waiting on the missing network half, not just on a caller. Sync moves bytes over links, so with
  // no transport in this shell a planner would plan a transfer that could not happen.
  plugins_sync_plan: 'the shell has no transport, and no caller for the planner either',
  plugins_sync_run: 'the shell has no transport, and no caller for the runner either',
  plugins_request_registry_search: 'no registry search is wired',
  // The plugin UI channel does not exist at all.
  plugin_ui_rpc: 'the plugin UI origin and its token injection do not exist',
  plugin_ui_ready: 'the plugin UI origin and its token injection do not exist'
}

/**
 * ★ What moved off this list, and why it is worth a note.
 *
 * `diag_logs_tail` was here with the reason "no log ring is behind AppState". That was true, and it was the *only* thing
 * standing in the way: the command itself is a few lines over a reader that already existed. The shell now starts its own
 * log, so the reason expired and the command moved up — which is what these reasons are for. A list of unbuilt things
 * with no reason attached rots into a list nobody can act on.
 */
