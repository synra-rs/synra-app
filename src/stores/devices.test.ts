/**
 * The device mirror, asserted as a mirror.
 *
 * # What these tests are careful not to assert
 *
 * Nothing about *policy*. There is no "should this device connect" here, because the store does not decide that — the
 * kernel does, and a test that asserted a rule would be a second copy of the rule. What is asserted is what the store
 * records*: that a snapshot replaces, that an event updates one card, and that a device the kernel stopped mentioning
 * stops being mentioned.
 *
 * This file is the first of two: what the store *records* is here, and what it *renders* — the paired/online lists —
 * is in `devices-lists.test.ts`. The factories both use are in `devices-test-helpers.ts`, which the runner does not
 * collect.
 */

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useDevicesStore } from './devices'
import { card, snapshot, transferSnapshot } from './devices-test-helpers'

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

  it('does not replace a newer full snapshot with a delayed identity read', async () => {
    const store = useDevicesStore()
    const previous = snapshot([]).identity
    let finish: ((value: unknown) => void) | undefined
    const call = vi.fn(() => new Promise((resolve) => {
      finish = resolve
    }))
    const reading = store.readIdentity({ call } as Parameters<typeof store.readIdentity>[0])
    store.applySnapshot({ ...snapshot([]), identity: { ...previous, displayName: 'Current device' } })
    finish!({ ok: true, value: previous })
    await reading
    expect(store.identity?.displayName).toBe('Current device')
  })

  it('only records the newest identity read', async () => {
    const store = useDevicesStore()
    const previous = snapshot([]).identity
    let finish: ((value: unknown) => void) | undefined
    const call = vi.fn()
      .mockImplementationOnce(() => new Promise((resolve) => {
        finish = resolve
      }))
      .mockResolvedValueOnce({ ok: true, value: { ...previous, displayName: 'New name' } })
    const reading = store.readIdentity({ call })
    await store.readIdentity({ call })
    finish!({ ok: true, value: previous })
    await reading
    expect(store.identity?.displayName).toBe('New name')
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
