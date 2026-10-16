/**
 * Routing an event to its handler, and unsubscribing from one.
 *
 * # What is being tested, and why it is not the tracker
 *
 * `@synra/protocol`'s `SequenceTracker` has its own tests. What this file asserts is the *dispatcher's* use of it: that
 * a repeat never reaches a store, that a gap asks for a snapshot rather than dropping the event silently, and that an
 * event this build does not know is treated as a reason to re-read rather than as nothing.
 *
 * That last one is the case an interface that predates its kernel is in, and it is the one a `switch`-based dispatcher
 * gets wrong by falling through to a default that does nothing.
 *
 * This file is the first of two: the routing decisions are here, and the sequence judgements —
 * `dispatcher-sequence.test.ts` — are beside it. The factories both use are in `dispatcher-test-helpers.ts`, which the
 * runner does not collect.
 */

import type { KernelEvent } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { EventDispatcher } from './dispatcher'

import { deviceUpdated, logLine, refresher } from './dispatcher-test-helpers'

describe('routing an event to its handler', () => {
  it('applies an event to the handler registered for its kind', () => {
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const seen: KernelEvent[] = []
    dispatcher.on('deviceCardUpdated', event => seen.push(event))

    expect(dispatcher.accept(deviceUpdated(1))).toEqual({ kind: 'applied' })
    expect(seen).toHaveLength(1)
    expect(seen[0]?.type).toBe('deviceCardUpdated')
  })

  it('does not call a handler registered for another kind', () => {
    // The map is keyed by the discriminant, so a store that subscribed to devices must not see a log line. A
    // dispatcher that broadcast every event would work until a store assumed its own shape.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const devices = vi.fn()
    dispatcher.on('deviceCardUpdated', devices)

    dispatcher.accept(logLine(1))
    expect(devices).not.toHaveBeenCalled()
  })

  it('calls every handler registered for one kind', () => {
    // Two stores may both care about a device: the list and the transfer panel. Registration replaces nothing.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const first = vi.fn()
    const second = vi.fn()
    dispatcher.on('deviceCardUpdated', first)
    dispatcher.on('deviceCardUpdated', second)

    dispatcher.accept(deviceUpdated(1))
    expect(first).toHaveBeenCalledTimes(1)
    expect(second).toHaveBeenCalledTimes(1)
  })

  it('hands the handler the event without the envelope', () => {
    // The envelope's `seq` and `at` belong to the channel. A handler that saw the sequence is one that could make a
    // second judgement about it, and two judgements about one sequence is how the interface disagrees with itself.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    let received: unknown
    dispatcher.on('logLine', (event) => {
      received = event
    })

    dispatcher.accept(logLine(7))
    expect(received).toEqual({ type: 'logLine', payload: { level: 'info', line: 'x' } })
    expect(received).not.toHaveProperty('seq')
  })

  it('stops calling a handler that unsubscribed', () => {
    // The unsubscribe function is what a component's cleanup calls, and a subscription that outlived its component
    // would write into a store for a page the user has left.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const handler = vi.fn()
    const off = dispatcher.on('deviceCardUpdated', handler)

    dispatcher.accept(deviceUpdated(1))
    off()
    dispatcher.accept(deviceUpdated(2))
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('tolerates unsubscribing twice', () => {
    // A cleanup that runs twice is ordinary — a component unmounted and its route also torn down — and an error there
    // would be a crash on the way out of a page.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const off = dispatcher.on('logLine', vi.fn())
    off()
    expect(() => {
      off()
    }).not.toThrow()
  })
})

describe('the handlers a store registers', () => {
  it('receives the payload with its own type', () => {
    // The compile-time half is the point: `event.payload.deviceId` compiles, and `event.payload.transferId` does not.
    // This test exists so the assertion is in the file rather than only in the type check.
    let deviceId: string | undefined
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('deviceCardUpdated', (event) => {
      deviceId = event.payload.deviceId
    })
    dispatcher.accept(deviceUpdated(1, 'dev_zzz'))
    expect(deviceId).toBe('dev_zzz')
  })
})
