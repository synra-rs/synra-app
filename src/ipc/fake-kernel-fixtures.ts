/**
 * The constants the fake kernel answers from.
 *
 * # Why this is a separate file, and why nothing else may import it
 *
 * The fake kernel's data tables are the largest part of it and the least interesting: what is worth reading in
 * `fake-kernel.ts` is the *behaviour* — the refusal shape, the listen-then-snapshot ordering, the scan that emits
 * events. The tables moved here so that file reads as behaviour, and they are only ever read by it.
 *
 * ★ `scripts/check-shapes.ts` has a `fake-quarantine` check: the fake is a fixture for tests, and the only file allowed
 * to name it is `kernel.ts`, whose job is choosing between it and the real bridge. These constants are part of the same
 * fixture, so they inherit the same rule — the fake's module is the only importer of this one, and importing it
 * somewhere else would put invented data in front of production code.
 *
 * ★ Each constant's *shape* is not a free choice: `src/ipc/commands.test.ts` holds every answer the fake gives against
 * the arity `crates/synra-ipc/src/command.rs` declares for that command (`Many` is a bare array, `One` is a single
 * object). A table here that disagrees with the contract is a fake that teaches the interface a shape the kernel never
 * sends — which is exactly how the plugins page came to crash on a real device while every test passed.
 */

import type {
  DeviceCard,
  IdentitySnapshot,
  IntentSnapshot,
  PluginSnapshot,
  TransferSnapshot
} from '@synra/protocol'

/** A device id that is shaped like one, so nothing here accidentally tests id parsing. */
export const SELF_ID = '01HQ0000000000000000000SELF'
export const PEER_ID = '01HQ0000000000000000000PEER'
export const UNKNOWN_ID = '01HQ00000000000000000UNKNOWN'

/** The identity of the machine running this interface. */
export const IDENTITY = {
  deviceId: SELF_ID,
  displayName: 'This device',
  platform: 'windows',
  kernelVersion: '0.1.0',
  nameIsDefault: true
} satisfies IdentitySnapshot

/** Two devices: one paired and reachable, one seen once and gone. */
export const DEVICES: readonly DeviceCard[] = [
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
export const PLUGINS: readonly PluginSnapshot[] = [
  {
    pluginId: '@synra-plugin/github',
    title: 'GitHub links',
    description: 'Opens a shared repository in the desktop browser.',
    version: '0.1.0',
    publisher: '@synra',
    enabled: true,
    state: 'active',
    hybrid: false,
    hasUi: false,
    health: {
      health: 'healthy',
      explanation: 'plugin.health.ok',
      callsAdmitted: 12,
      callsRefused: 0,
      usage: null
    },
    granted: ['platform:open-url', 'net:read-title'],
    denied: [],
    // ★ The manifest's list, which is what the consent checkboxes are built from. Without it the fake kernel's screen
    // shows no prompt at all — the one place the browser build is supposed to demonstrate the consent flow.
    requested: ['platform:open-url', 'net:read-title']
  },
  {
    pluginId: '@synra-plugin/shortcuts',
    title: 'Shortcuts',
    version: '0.2.0',
    publisher: '@synra',
    enabled: true,
    state: 'faulted',
    hybrid: false,
    hasUi: false,
    health: {
      health: 'faulted',
      explanation: 'plugin.health.quotaExceeded',
      callsAdmitted: 40,
      callsRefused: 3,
      usage: null
    },
    granted: ['platform:open-url'],
    denied: ['fs:read', 'net:fetch'],
    requested: ['platform:open-url', 'fs:read', 'net:fetch'],
    update: {
      version: '0.3.0',
      needsNewConsent: true,
      addedCapabilities: ['net:fetch:api.github.com']
    }
  }
]

/** One intent that settled, so the home page has something true to say. */
export const INTENT: IntentSnapshot = {
  intentId: '01HQ00000000000000000INTNT1',
  state: 'succeeded',
  pluginId: '@synra-plugin/github',
  candidates: [],
  startedAtMs: 0
}

/**
 * What the board is holding, for the listing read.
 *
 * The same intent the single read answers about, because it is the same board: a fake whose list and whose single read
 * disagreed would be teaching the interface that the two commands describe different things, which is the very mistake
 * `HomePage.vue` made before `intent_list` existed.
 */
export const INTENTS: readonly IntentSnapshot[] = [INTENT]

/** No transfers: an empty list is a real answer and the panel has to render it. */
export const TRANSFERS: readonly TransferSnapshot[] = []
