/**
 * What the devices store hands a screen to render.
 *
 * # What these tests are careful not to assert
 *
 * Nothing about *policy*. There is no "should this device connect" here, because the store does not decide that — the
 * kernel does, and a test that asserted a rule would be a second copy of the rule. What is asserted is what the store
 * records*: that a snapshot replaces, that an event updates one card, and that a device the kernel stopped mentioning
 * stops being mentioned.
 *
 * This file is the second of two: what the store *records* is in `devices.test.ts`, and the derived lists a screen
 * reads are here. The factories both use are in `devices-test-helpers.ts`, which the runner does not collect.
 */

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useDevicesStore } from './devices'
import { card, snapshot } from './devices-test-helpers'

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
