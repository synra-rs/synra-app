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
  // Pairing: the request dials the address the scan recorded, the answer travels back over the same link, and the desk
  // carries a request from the socket thread that received it to the command that answers it.
  'pairing_request',
  'pairing_respond',
  'pairing_cancel',
  'plugins_list',
  'plugins_preview',
  'plugins_catalog_search',
  // ★ The contract's simpler search entry: the same endpoint, reached with only the text. It was on the not-yet list
  // because nothing wired it; the endpoint it needs has existed since `plugins_catalog_search` landed.
  'plugins_request_registry_search',
  'plugins_enable',
  'plugins_disable',
  'plugins_consent_respond',
  // ★ The install path, which moved from "no installer is wired" to registered when the pipeline became a kernel
  // function: `synra_core::install` is the same one the daemon runs.
  'plugins_install',
  // ★ Removable, with the data decision passed through rather than defaulted in two places.
  'plugins_uninstall',
  // ★ The contract declares it (`PluginUi`, **Host**), and this is the only command that creates a WebView. A plugin page
  // that could call it would be one that can spawn windows, which is not a capability this model grants.
  'plugin_ui_open',
  // The page's own report, checked against the session the host issued for it.
  'plugin_ui_ready',
  // ★ The plugin page's one call channel: a token the host issued, then a method from the surface `PluginUiOp` names.
  // `device.list` and `storage.*` have bodies; everything else on the surface refuses with `unsupported` until it is
  // built, rather than answering an empty list.
  'plugin_ui_rpc',
  // The transfers in flight. The registry is filled by the two send commands as they move blocks; an **inbound** transfer is
  // driven by the listener and is not reported in this list yet.
  'transfer_cancel',
  'transfer_respond',
  'transfer_send_file',
  'transfer_send_text',
  'transfer_list',
  // ★ The audit trail: the ring had no writer until a plugin instance was given a session.
  'plugins_audit',
  // The other half of that section: 18 §5.2 asks to clear one plugin's history, and reading it was only half the item.
  'plugins_audit_clear',
  // Planning a sync: asks every paired device what it holds (one small request/response) and records the answers, so the plan
  // is about now rather than about the last bundle that happened to move.
  'plugins_sync_plan',
  // ★ Running the plan: one dial and one transfer per device that needs the artifact, and the receiver's answer decides
  // what `sync_records` is allowed to say. A device that could not be reached is a report entry, not a failed command.
  'plugins_sync_run',
  'plugins_publishers',
  'plugins_publisher_decide',
  'plugins_check_update',
  'plugins_update',
  // ★ Retry a faulted plugin without flipping its switches: the row is already enabled and trusted, what failed was
  // starting it, and "turn it off and on again" was the only way to ask.
  'plugins_reload',
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
  'diag_probes',
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
  // ★ The network half this comment used to name is here now — the shell listens (`listener.rs`), dials for pairing and for  // transfers, and asks peers what they hold for a sync plan. What is still missing is not a transport but the code that
  // acts on a plan: no runner sends a plugin bundle, and the contract's name for that is `plugins_sync_run`.
  //
  // Pairing said "needs the pairing desk wired to a Tauri command" — and the desk is in `AppState` now, answered from a
  // command while the request waits on a socket thread. Transfers said the same sort of thing; all four are registered.
  //
  // What remains is per command below, and each reason says which layer is missing rather than pointing at a whole half.
  // ★ The four intent commands are registered now, and the reason is worth keeping: they used to carry "needs the
  // intent board behind AppState", which was false — the board was already in the state. What was missing was the
  // orchestration, which lived in `synra-daemon/src/runtime/intent.rs` **inside a binary**. It is in
  // `synra_core::dispatch::orchestrate` now, and the shell's commands are the thin layer that turns DTOs into calls.
  // Transfers are implemented in the kernel but this shell has nothing to send them over.
  // Connecting and disconnecting are deliberately absent — the kernel dials, the UI does not.
  //
  // ★ "The kernel dials" is still true of the daemon and not of this shell, which never dials anything. The reason stands
  // as an intent — the UI is not where a dial belongs — but it is not a claim that dialling happens here.
  devices_connect: 'deliberately absent: the kernel dials, the UI does not',
  devices_disconnect: 'deliberately absent: the kernel dials, the UI does not'
  // The plugin lifecycle beyond enable/disable/install is not built. `plugins_install` moved out of this list when the
  // pipeline became reachable from the shell: `synra_core::install` is the daemon's pipeline too, so the order of the
  // checks has one implementation and this shell supplies only what it can do (a desktop can open a window).
}

/**
 * ★ What moved off this list, and why it is worth a note.
 *
 * `diag_logs_tail` was here with the reason "no log ring is behind AppState". That was true, and it was the *only* thing
 * standing in the way: the command itself is a few lines over a reader that already existed. The shell now starts its own
 * log, so the reason expired and the command moved up — which is what these reasons are for. A list of unbuilt things
 * with no reason attached rots into a list nobody can act on.
 */
