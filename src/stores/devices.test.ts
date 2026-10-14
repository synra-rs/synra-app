/**
 * The device mirror, asserted as a mirror.
 *
 * # What these tests are careful not to assert
 *
 * Nothing about *policy*. There is no "should this device connect" here, because the store does not decide that — the
 * kernel does, and a test that asserted a rule would be a second copy of the rule. What is asserted is what the store
 * records*: that a snapshot replaces, that an event updates one card, and that a device the kernel stopped mentioning
 * stops being mentioned.
 */

import type { DeviceCard, DevicesSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useDevicesStore } from './devices'

/** A card with the given id and nothing interesting. */
function card(deviceId: string, overrides: Partial<DeviceCard> = {}): DeviceCard {
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
function snapshot(devices: DeviceCard[], scanning?: DevicesSnapshot['scanning']): DevicesSnapshot {
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

describe('recording a snapshot', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('replaces the list rather than merging it', () => {
    // ★ A snapshot is the kernel's complete answer, so a card it does not carry is one that is gone. Merging would keep
    // a device the user removed until the interface restarted — and the user would see it come back after every
    // reconnect.
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a'), card('dev_b')]))
    expect(store.cards).toHaveLength(2)

    store.applySnapshot(snapshot([card('dev_a')]))
    expect(store.cards.map(each => each.deviceId)).toEqual(['dev_a'])
  })

  it('records the identity, so a screen needs one call rather than two', () => {
    const store = useDevicesStore()
    store.applySnapshot(snapshot([]))
    expect(store.identity?.displayName).toBe('This machine')
    expect(store.identity?.deviceId).toBe('dev_self')
  })

  it('records a scan as the scan itself rather than as a flag', () => {
    // The kernel's `scanning` carries the progress. A store that kept a boolean beside it would be two places the
    // answer lives, and the day they disagree the interface shows "scanning" with a stale percentage.
    const store = useDevicesStore()
    store.applySnapshot(
      snapshot([], { scanId: 'scan_1', elapsedMs: 500, totalMs: 3_000, found: 2 })
    )
    expect(store.scanning).toBe(true)
    expect(store.scan?.found).toBe(2)
    expect(store.scan?.totalMs).toBe(3_000)

    store.applySnapshot(snapshot([]))
    expect(store.scanning).toBe(false)
    expect(store.scan).toBeUndefined()
  })

  it('indexes the cards by id for the paths that look one up per request', () => {
    // The pairing dialog looks up on every incoming request, which arrives at the peer's rate rather than the user's,
    // so the index is maintained rather than derived per read.
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a'), card('dev_b')]))
    expect(store.get('dev_b')?.deviceId).toBe('dev_b')
    expect(store.get('dev_missing')).toBeUndefined()
  })
})

describe('applying one event', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('adds a device it has not seen', () => {
    const store = useDevicesStore()
    store.applySnapshot(snapshot([]))
    expect(store.apply({ type: 'deviceCardUpdated', payload: card('dev_new') })).toBe(true)
    expect(store.get('dev_new')?.deviceId).toBe('dev_new')
  })

  it('updates the one card it is about and leaves the others alone', () => {
    // ★ The property that makes an incremental event worth having: a store that rebuilt the list from the event would
    // lose every device the event did not mention, which is all of them but one.
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a'), card('dev_b'), card('dev_c')]))

    store.apply({
      type: 'deviceCardUpdated',
      payload: card('dev_b', { displayName: 'Renamed', online: false })
    })

    expect(store.get('dev_b')?.displayName).toBe('Renamed')
    expect(store.get('dev_b')?.online).toBe(false)
    expect(store.cards.map(each => each.deviceId)).toEqual(['dev_a', 'dev_b', 'dev_c'])
    expect(store.get('dev_a')?.displayName).toBe('Device dev_a')
  })

  it('removes a device the kernel stopped mentioning', () => {
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a'), card('dev_b')]))
    expect(store.apply({ type: 'deviceRemoved', payload: { deviceId: 'dev_a' } })).toBe(true)
    expect(store.cards.map(each => each.deviceId)).toEqual(['dev_b'])
    expect(store.get('dev_a')).toBeUndefined()
  })

  it('records a scan update without a snapshot', () => {
    // A scan is the event that arrives most often while the user is waiting, and it is the one that must not need a
    // round trip to show progress.
    const store = useDevicesStore()
    store.applySnapshot(snapshot([]))
    expect(
      store.apply({
        type: 'scanUpdated',
        payload: { scanId: 'scan_1', elapsedMs: 100, totalMs: 3_000, found: 1 }
      })
    ).toBe(true)
    expect(store.scanning).toBe(true)
    expect(store.scan?.found).toBe(1)
  })

  it('says no to an event that is not its own', () => {
    // So a caller wiring the dispatcher can tell a routed event from an unhandled one without a second lookup.
    const store = useDevicesStore()
    expect(
      store.apply({ type: 'transferUpdated', payload: transferSnapshot('t1') })
    ).toBe(false)
  })
})

describe('a transfer on a device', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('attaches to the card it belongs to', () => {
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a'), card('dev_b')]))
    store.setTransfer('dev_a', { transferId: 't1', direction: 'send', progress: 0.5 })

    expect(store.get('dev_a')?.transfer?.transferId).toBe('t1')
    expect(store.get('dev_b')?.transfer).toBeUndefined()
  })

  it('clears the transfer without disturbing the rest of the card', () => {
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a', { displayName: 'Laptop', address: '10.0.0.5' })]))
    store.setTransfer('dev_a', { transferId: 't1', direction: 'send', progress: 0.5 })
    store.setTransfer('dev_a', undefined)

    const found = store.get('dev_a')
    expect(found?.transfer).toBeUndefined()
    // The card's own fields survive: a `setTransfer` that cleared the transfer by replacing the card with a fresh one
    // would lose the name and the address, and the row would go blank at the end of a transfer.
    expect(found?.displayName).toBe('Laptop')
    expect(found?.address).toBe('10.0.0.5')
  })

  it('drops a transfer for a device the list does not have', () => {
    // Rather than inventing a card: a card with no name or platform renders as a blank row, and the snapshot that
    // answers the gap will carry both.
    const store = useDevicesStore()
    store.applySnapshot(snapshot([card('dev_a')]))
    store.setTransfer('dev_unknown', { transferId: 't1', direction: 'send', progress: 0.5 })
    expect(store.cards.map(each => each.deviceId)).toEqual(['dev_a'])
  })
})

describe('the lists a screen renders', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('separates paired from merely-seen, and online from merely-paired', () => {
    // ★ Two questions, not one. A paired device that is offline is one the user can send to later; a device that is
    // online and unpaired is one they have to pair with first. Collapsing them would make the list show a device as
    // usable when it is not.
    const store = useDevicesStore()
    store.applySnapshot(
      snapshot([
        card('dev_paired_online', { paired: true, online: true }),
        card('dev_paired_offline', { paired: true, online: false }),
        card('dev_unpaired_online', { paired: false, online: true }),
        card('dev_unpaired_offline', { paired: false, online: false })
      ])
    )

    expect(store.paired.map(each => each.deviceId)).toEqual([
      'dev_paired_online',
      'dev_paired_offline'
    ])
    expect(store.online.map(each => each.deviceId)).toEqual(['dev_paired_online'])
  })

  it('is empty before the first snapshot rather than undefined', () => {
    // The panel renders on mount, before the kernel has answered. A store that started as `undefined` would make every
    // component handle a case that is not a state.
    const store = useDevicesStore()
    expect(store.cards).toEqual([])
    expect(store.paired).toEqual([])
    expect(store.online).toEqual([])
    expect(store.scanning).toBe(false)
  })
})

/** A transfer snapshot, for the one test about an event that is not the devices store's. */
function transferSnapshot(transferId: string) {
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
