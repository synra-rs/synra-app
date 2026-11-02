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
  // Pairing: the code, the answer and the withdrawal, all on the one desk the kernel owns.
  'devices_pair',
  'pairing_respond',
  'pairing_cancel',
  'pairing_snapshot',
  // Pairing uses the shared restricted service with explicit certificate imports and a readable task snapshot.
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
  // ★ The files that already landed; the in-flight list cannot answer this because a finished row is removed.
  'transfer_received',
  // ★ Open one of those files with the platform's application; the shell checks it is in the inbox first.
  'transfer_open',
  // ★ Show the file in the file manager; the same inbox containment check as opening it.
  'transfer_reveal',
  // ★ Remove a received file; destructive, so the page asks first and the shell checks the path is in the inbox.
  'transfer_discard',
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
  'plugins_updates',
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
  // ★ The crash prompt's "ignore": deleting the reports is what makes the next start normal, and the button is in the
  // diagnostics section beside the export that already carries them.
  'diag_crash_dismiss',
  // ★ The "查看详情" half: the reports themselves, newest first, read on request rather than on mount.
  'diag_crashes',
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
  // ★ Empty, and that is the state to keep it in. Every name the contract declares is registered here now:
  // pairing and credentials left the contract entirely (ADR-025), and `devices_connect`/`devices_disconnect`
  // were deleted because nothing was ever going to implement them — the kernel dials when it has something
  // to send, and a "connect" button would only be a dial with no payload.
  //
  // The guard in `registration.test.ts` still reads this map, so a future command that is declared before it
  // is implemented has one place to wait with its reason attached.
}

/**
 * ★ What moved off this list, and why it is worth a note.
 *
 * `diag_logs_tail` was here with the reason "no log ring is behind AppState". That was true, and it was the *only* thing
 * standing in the way: the command itself is a few lines over a reader that already existed. The shell now starts its own
 * log, so the reason expired and the command moved up — which is what these reasons are for. A list of unbuilt things
 * with no reason attached rots into a list nobody can act on.
 */
