/**
 * The factories every devices-store test is built from.
 *
 * # Why this is not a `.test.ts`
 *
 * The runner collects `src/**\/*.test.ts` and treats each collected file as a suite. A helper module that is collected
 * runs its own empty suite and, worse, reads as a file of tests that asserts nothing. It is named for what it holds —
 * fixtures and factories — so nobody goes looking for assertions in it.
 */

import type { DeviceCard, DevicesSnapshot } from '@synra/protocol'

/** A card with the given id and nothing interesting. */
export function card(deviceId: string, overrides: Partial<DeviceCard> = {}): DeviceCard {
  return {
    deviceId,
    displayName: `Device ${deviceId}`,
    platform: 'windows',
    online: true,
    paired: true,
    blocked: false,
    lastSeenMs: 1_000,
    ...overrides
  }
}

/** A snapshot with the given cards. */
export function snapshot(devices: DeviceCard[], scanning?: DevicesSnapshot['scanning']): DevicesSnapshot {
  return {
    identity: {
      deviceId: 'dev_self',
      displayName: 'This machine',
      platform: 'linux',
      kernelVersion: '0.1.0',
      nameIsDefault: false
    },
    devices,
    ...(scanning === undefined ? {} : { scanning })
  }
}

/** A transfer snapshot, for the one test about an event that is not the devices store's. */
export function transferSnapshot(transferId: string) {
  return {
    transferId,
    peerDevice: 'dev_a',
    peerName: 'Laptop',
    direction: 'send',
    kind: 'attachment',
    state: 'active',
    fileName: 'a.txt',
    receivedBytes: 1,
    totalBytes: 2,
    blocksDone: 1,
    blocksTotal: 2,
    startedAtMs: 1
  }
}
