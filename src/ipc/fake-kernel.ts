/**
 * A kernel that is not there.
 *
 * # What this is for
 *
 * The roadmap puts a fake kernel in M0 so that the interface is not the thing that is waiting at the end. Four pages that
 * render against snapshots are worth building now, because every later milestone then has a place to put its work
 * instead of inventing one.
 *
 * # Why it implements `Bridge` and nothing else
 *
 * `Bridge` is the named seam `KernelClient` calls through. A fake that implemented the client itself would be testing a
 * fake — the parts worth testing are the error mapping, the retry classification and the listen-then-snapshot ordering,
 * and all three live *below* `Bridge`. So the fake is a bridge, and everything above it is the real code.
 *
 * # Why it answers real command names
 *
 * `scripts/check-shapes.ts` compares every command this repository names against the kernel's own list, and a name that
 * drifts is a call that fails at runtime with "command not found" and points at neither file. A fake that invented
 * `getDevices` would pass every test and teach the interface a command the kernel does not have — so this one answers
 * the 40 names that are declared.
 *
 * # Why the data is in a file beside this one
 *
 * The tables were the longest part of this file and the least informative: a reader looking for what the fake *does* had
 * to scroll past a device list to find it. They are in `fake-kernel-fixtures.ts`, which only this file imports — the
 * `fake-quarantine` check in `scripts/check-shapes.ts` allows one production file to name the fake, and this is it.
 */

import type { DevicesSnapshot, SequencedKernelEvent } from '@synra/protocol'

import type { Bridge } from './client'
import { KERNEL_EVENT_CHANNEL } from './channel'
import {
  DEVICES,
  IDENTITY,
  INTENT,
  INTENTS,
  PLUGINS,
  TRANSFERS
} from './fake-kernel-fixtures'

/**
 * The arguments the shell requires, per command — the half of the contract this fake used to ignore.
 *
 * ★ **Why the fake checks this at all.**
 *
 * `#[tauri::command]` deserializes every argument *before* the command body runs, so a call that is missing one is
 * rejected by the bridge — `invalid args \`opts\` for command \`devices_scan_start\`: command devices_scan_start missing
 * required key opts` — and a rejection is not a refusal value. `DevicesPage.vue` called `devices_scan_start` with no
 * arguments at all for as long as it did because this fake answered anyway: every test was green, `pnpm dev` scanned
 * happily, and the first run against the real shell said "The interface could not reach the kernel". A fake that is
 * **more permissive** than the thing it stands in for is the one failure it cannot catch, so the names here are the
 * shell's own (`src-tauri/src/shell.rs`), and `scripts/check-shapes.ts` compares this table against those wrapper
 * signatures in both directions.
 *
 * The keys are camelCase because that is the rename rule `#[tauri::command]` applies to a parameter name: `scan_id` on the
 * Rust side is `scanId` over the wire.
 */
export const REQUIRED_ARGS: Readonly<Record<string, readonly string[]>> = {
  devices_scan_start: ['opts'],
  devices_scan_cancel: ['scanId'],
  intent_snapshot: ['intentId'],
  transfer_respond: ['transferId', 'accept'],
  plugins_publisher_decide: ['publisherId', 'trust'],
  plugins_preview: ['pluginId'],
  plugins_check_update: ['pluginId'],
  plugins_update: ['pluginId'],
  plugins_catalog_search: ['query'],
  plugins_request_registry_search: ['query']
}

/**
 * A `Bridge` that answers from constants.
 *
 * # Why the constants are copied per call rather than shared
 *
 * `applySnapshot` replaces the array it is given, and a component that sorted one in place would edit the constant that
 * every later call returns. Copying costs three devices and makes the fake safe to hold across tests.
 */
export class FakeKernel implements Bridge {
  /** Handlers by channel, so `listen` can hand back an unsubscribe that actually unsubscribes. */
  readonly #handlers = new Map<string, Set<(payload: unknown) => void>>()

  /** Monotonic, because the dispatcher's judgement is a function of it. */
  #seq = 0

  /** The scan's timer, so a second `devices_scan_start` does not leave the first one running. */
  #scanTimer: ReturnType<typeof setInterval> | undefined

  /**
   * Refuses a call that is missing an argument the shell requires.
   *
   * The message is the one `tauri::Error::InvalidArgs` formats: Tauri rejects the invoke with that string, and the client
   * classifies a rejection that is not an `IpcError` as `error.bridgeFailed` — which is the sentence the user saw. It is
   * an `Error` here rather than a bare string because this is TypeScript, and `bridgeFailure` reads `thrown.message`
   * either way, so the interface cannot tell the difference.
   */
  #requireArguments(command: string, args?: Readonly<Record<string, unknown>>): void {
    for (const key of REQUIRED_ARGS[command] ?? []) {
      if (args?.[key] === undefined) {
        throw new Error(
          `invalid args \`${key}\` for command \`${command}\`: command ${command} missing required key ${key}`
        )
      }
    }
  }

  async invoke(command: string, args?: Readonly<Record<string, unknown>>): Promise<unknown> {
    // First, because this is the order the shell has: the bridge deserializes the arguments and only then runs the
    // command. See `REQUIRED_ARGS`.
    this.#requireArguments(command, args)

    switch (command) {
      case 'identity_get':
        return { ...IDENTITY }
      case 'devices_snapshot':
        return this.#devicesSnapshot()
      case 'devices_scan_start': {
        // ★ The mode matters, and this fake used to scan whatever it was asked for.
        //
        // A cached scan is over before it answers and publishes nothing, which is what the shell answers today; a network
        // scan is the one that reports itself as events. A fake that animated for both made the only mode a real shell
        // can currently answer invisible in every test — and made the page's two calls look like one call.
        const opts = args?.opts as { readonly cachedOnly?: boolean } | undefined
        if (opts?.cachedOnly === true) {
          // Nothing to wait for and nothing to publish: the scan is over before the answer, which is what the shell
          // answers today.
          this.#stopScan()
          return null
        }
        this.#startScan()
        // `null` is the divergence `commands.test.ts` records: the shell answers with the new scan's id and this fake does
        // not invent one, because the page reads only `ok` and the arity the contract declares is `One`.
        return null
      }
      case 'devices_scan_cancel':
        this.#stopScan()
        return null
      case 'plugins_publishers':
        // ★ A bare array, `Many`: an empty list is a valid answer, not a missing one.
        return []
      case 'plugins_catalog_search':
      case 'plugins_request_registry_search':
        // A search that finds nothing is an answer, not a refusal; the page draws its own empty state.
        return []
      case 'plugins_publisher_decide':
        return {}
      case 'plugins_preview':
        // The permissions a preview shows, in the kernel's own shape. A fake that answered nothing would make the
        // confirmation dialog untestable in every page that opens it.
        return {
          pluginId: '@acme/hello',
          version: '1.0.0',
          title: 'Hello',
          publisher: '@acme',
          capabilities: ['log:*'],
          headless: true,
          hasUi: false
        }
      case 'plugins_check_update':
        // `Maybe`: "no update" is `null`, and the fake answers the ordinary case rather than inventing one.
        return null
      case 'plugins_update':
        return null
      case 'plugins_list':
        // ★ A bare array, because the kernel's own table says this command returns `Many`.
        //
        // `crates/synra-ipc/src/command.rs` declares the payload's arity per command: `"plugins_list" => Plugins, Host,
        // Many, false`, and the shell's signature is `Result<Vec<PluginSnapshot>, IpcError>`. `Many` means the payload
        // *is* the list — there is no envelope field to read it from.
        //
        // This used to answer `{ plugins: […] }`. That shape matched an earlier `PluginsPage.vue` which read
        // `answer.value.plugins`, so every test passed and the real app crashed with `t.value is not iterable` the
        // moment it ran against a real kernel. A fake that agrees with the interface instead of with the contract is
        // the one failure mode a fake cannot catch by itself — which is why `commands.test.ts` now asserts every shape
        // here against `command.rs`.
        return PLUGINS.map(plugin => ({ ...plugin }))
      case 'intent_list':
        // ★ A bare array, because the contract says this command returns `Many` — the shape the home page got wrong.
        //
        // The page used to call `intent_snapshot` (which takes an identifier and answers about *one* intent) with no
        // arguments and read `answer.value.intents`, an envelope no command sends. `intent_list` is the command that was
        // missing, and this is what it answers with.
        return INTENTS.map(intent => ({ ...intent }))
      case 'intent_snapshot':
        // `One`, and it takes an `intent_id`: the command answers about *one* intent rather than listing them.
        return { ...INTENT }
      case 'transfer_respond':
        // The answer is nothing; the row leaves through the `transferFinished` event the kernel will publish.
        return {}
      case 'transfer_list':
        // ★ A bare array, for the same reason as `plugins_list` above: `Many`.
        return TRANSFERS.map(transfer => ({ ...transfer }))
      case 'config_get':
        // ★ The kernel's own shape, not a flat bag of settings.
        //
        // This used to answer `{ theme: 'system', autoSyncPlugins: true }`. The kernel answers
        // `{ values: {…}, overridden: […] }`, because the boundary carries a bag and the *screens* own their own
        // shape — so a fake with the flat form teaches the interface to read a field that never arrives, and the
        // failure is a settings page that renders empty with nothing in the log.
        //
        // The two keys are the kernel's own `ui.*` dotted paths, and `overridden` lists what the user changed rather
        // than what the defaults happen to be.
        return {
          values: { 'ui.theme': 'system', 'ui.autoSyncPlugins': true },
          overridden: ['ui.theme']
        }
      default:
        // A refusal, in the kernel's own shape, rather than a rejection: the client checks for it and a fake that
        // rejected would teach the interface that an unknown command throws. `unsupported` because that is what this
        // kernel is saying — it does not have that command — rather than that the argument was malformed.
        return {
          code: 'unsupported',
          message: 'error.commandNotFound',
          params: { command },
          retryable: false
        }
    }
  }

  async listen(event: string, handler: (payload: unknown) => void): Promise<() => void> {
    let handlers = this.#handlers.get(event)
    if (handlers === undefined) {
      handlers = new Set()
      this.#handlers.set(event, handlers)
    }
    handlers.add(handler)
    // A function rather than a bare `delete`, because a caller that unsubscribes twice must not remove a handler some
    // other listener still owns.
    return () => {
      handlers?.delete(handler)
    }
  }

  /** The device list as the kernel spells it: `devices`, not `cards`, and the identity alongside. */
  #devicesSnapshot(): DevicesSnapshot {
    return {
      identity: { ...IDENTITY },
      devices: DEVICES.map(device => ({ ...device }))
    }
  }

  /**
   * A scan that takes about a second and finds one device on the way.
   *
   * The point is the *events*, not the timing: a page that renders correctly against a static snapshot proves nothing
   * about the listen-then-snapshot ordering or the sequence tracker, and those are the parts that are hardest to notice
   * being wrong.
   */
  #startScan(): void {
    this.#stopScan()
    const scanId = '01HQ00000000000000000SCAN01'
    const totalMs = 900
    const startedAt = Date.now()
    this.#emit('scanUpdated', { scanId, elapsedMs: 0, totalMs, found: 0 })

    this.#scanTimer = setInterval(() => {
      const elapsedMs = Date.now() - startedAt
      if (elapsedMs >= totalMs) {
        this.#stopScan()
        this.#emit('scanUpdated', { scanId, elapsedMs: totalMs, totalMs, found: 1 })
        this.#emit('deviceCardUpdated', { ...DEVICES[1]!, lastSeenMs: Date.now() })
        return
      }
      this.#emit('scanUpdated', { scanId, elapsedMs, totalMs, found: 0 })
    }, 300)
  }

  #stopScan(): void {
    if (this.#scanTimer !== undefined) {
      clearInterval(this.#scanTimer)
      this.#scanTimer = undefined
    }
  }

  /**
   * Publishes one event the way the kernel would.
   *
   * Flattened, because `KernelEventEnvelope` and `KernelEvent` are one object on the wire — the interface's dispatcher
   * reads `seq` off the same value it switches on.
   */
  #emit(type: SequencedKernelEvent['type'], payload: unknown): void {
    this.#seq += 1
    const event = { type, payload, seq: this.#seq, at: Date.now() }
    for (const handler of this.#handlers.get(KERNEL_EVENT_CHANNEL) ?? []) {
      handler(event)
    }
  }
}
