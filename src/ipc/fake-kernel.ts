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
 * the 39 names that are declared.
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
  PLUGINS,
  TRANSFERS
} from './fake-kernel-fixtures'

/**
 * A `Bridge` that answers from constants.
 *
 * # Why the constants are copied per call rather than shared
 *
 * `applySnapshot` replaces the array it is given, and a component that sorted one in place would edit the constant that
 * every later call returns. Copying costs three devices and makes the fake safe to hold across tests.
 */export class FakeKernel implements Bridge {
  /** Handlers by channel, so `listen` can hand back an unsubscribe that actually unsubscribes. */
  readonly #handlers = new Map<string, Set<(payload: unknown) => void>>()

  /** Monotonic, because the dispatcher's judgement is a function of it. */
  #seq = 0

  /** The scan's timer, so a second `devices_scan_start` does not leave the first one running. */
  #scanTimer: ReturnType<typeof setInterval> | undefined

  async invoke(command: string, _args?: Readonly<Record<string, unknown>>): Promise<unknown> {
    switch (command) {
      case 'identity_get':
        return { ...IDENTITY }
      case 'devices_snapshot':
        return this.#devicesSnapshot()
      case 'devices_scan_start':
        this.#startScan()
        return null
      case 'devices_scan_cancel':
        this.#stopScan()
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
      case 'intent_snapshot':
        // `One`, and it takes an `intent_id`: the command answers about *one* intent rather than listing them.
        return { ...INTENT }
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
