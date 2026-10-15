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
 */

import type {
  DeviceCard,
  DevicesSnapshot,
  IdentitySnapshot,
  IntentSnapshot,
  PluginSnapshot,
  SequencedKernelEvent,
  TransferSnapshot
} from '@synra/protocol'

import type { Bridge } from './client'

/**
 * The channel kernel events arrive on.
 *
 * One channel for every event rather than one per kind: the sequence number is a property of the stream, and a
 * per-kind channel would have one sequence per kind, so a gap in one would look like a fresh start in the next.
 */
export const KERNEL_EVENT_CHANNEL = 'kernel-event'

/** A device id that is shaped like one, so nothing here accidentally tests id parsing. */
const SELF_ID = '01HQ0000000000000000000SELF'
const PEER_ID = '01HQ0000000000000000000PEER'
const UNKNOWN_ID = '01HQ00000000000000000UNKNOWN'

/** The identity of the machine running this interface. */
const IDENTITY = {
  deviceId: SELF_ID,
  displayName: 'This device',
  platform: 'windows',
  kernelVersion: '0.1.0',
  nameIsDefault: true
} satisfies IdentitySnapshot

/** Two devices: one paired and reachable, one seen once and gone. */
const DEVICES: readonly DeviceCard[] = [
  {
    deviceId: PEER_ID,
    displayName: 'Pixel 8',
    platform: 'android',
    online: true,
    paired: true,
    blocked: false,
    lastSeenMs: 0
  },
  {
    deviceId: UNKNOWN_ID,
    displayName: 'unknown-android',
    platform: 'android',
    online: false,
    paired: false,
    blocked: false,
    lastSeenMs: 0,
    address: '192.168.1.57'
  }
]

/**
 * Two plugins, and the second one is the interesting one.
 *
 * `faulted` while `enabled` is what the design document keeps insisting on: a state that is neither "on" nor "off" and
 * that must not be *derived* from the other two. A fake that only had healthy plugins would let that rule stay
 * untested until it was expensive.
 */
const PLUGINS: readonly PluginSnapshot[] = [
  {
    pluginId: '@synra-plugin/github',
    title: 'GitHub links',
    description: 'Opens a shared repository in the desktop browser.',
    version: '0.1.0',
    publisher: '@synra',
    enabled: true,
    state: 'active',
    hybrid: false,
    health: {
      health: 'healthy',
      explanation: 'plugin.health.ok',
      callsAdmitted: 12,
      callsRefused: 0,
      usage: null
    },
    granted: ['platform:open-url', 'net:read-title'],
    denied: []
  },
  {
    pluginId: '@synra-plugin/shortcuts',
    title: 'Shortcuts',
    version: '0.2.0',
    publisher: '@synra',
    enabled: true,
    state: 'faulted',
    hybrid: false,
    health: {
      health: 'faulted',
      explanation: 'plugin.health.quotaExceeded',
      callsAdmitted: 40,
      callsRefused: 3,
      usage: null
    },
    granted: ['platform:open-url'],
    denied: ['fs:read', 'net:fetch'],
    update: {
      version: '0.3.0',
      needsNewConsent: true,
      addedCapabilities: ['net:fetch:api.github.com']
    }
  }
]

/** One intent that settled, so the home page has something true to say. */
const INTENTS: readonly IntentSnapshot[] = [
  {
    intentId: '01HQ00000000000000000INTNT1',
    state: 'succeeded',
    pluginId: '@synra-plugin/github',
    candidates: [],
    startedAtMs: 0
  }
]

/** No transfers: an empty list is a real answer and the panel has to render it. */
const TRANSFERS: readonly TransferSnapshot[] = []

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
        return { plugins: PLUGINS.map(plugin => ({ ...plugin })) }
      case 'intent_snapshot':
        return { intents: INTENTS.map(intent => ({ ...intent })) }
      case 'transfer_list':
        return { transfers: TRANSFERS.map(transfer => ({ ...transfer })) }
      case 'config_get':
        // The shape is the kernel's, and a settings page that invents its own key names is a settings page that has to
        // be rewritten. One real key, so the round trip is exercised.
        return { theme: 'system', autoSyncPlugins: true }
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
