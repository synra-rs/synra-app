/**
 * The dispatcher's judgement of the sequence, and of the events it does not know.
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
 * This file is the second of two: the routing decisions are in `dispatcher-routing.test.ts`, and the factories both use
 * are in `dispatcher-test-helpers.ts`, which the runner does not collect.
 */

import type { SequencedKernelEvent } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { EventDispatcher } from './dispatcher'

import { deviceUpdated, logLine, refresher } from './dispatcher-test-helpers'

describe('judging the sequence before the event', () => {
  it('ignores a repeat without calling a handler', () => {
    // ★ A channel delivers the same event twice around a reload, and applying it twice is what makes a device appear
    // once in the list and be fetched twice.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const handler = vi.fn()
    dispatcher.on('deviceCardUpdated', handler)

    expect(dispatcher.accept(deviceUpdated(5))).toEqual({ kind: 'applied' })
    expect(dispatcher.accept(deviceUpdated(5))).toEqual({ kind: 'ignored' })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(refresh).not.toHaveBeenCalled()
  })

  it('does not advance the sequence on a repeat', () => {
    // Advancing would make the next contiguous event look like a gap, and the interface would re-read for no reason.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    // A handler is registered because an event with none is `unknown` — correctly, since the picture would be
    // incomplete — and this test is about the sequence rather than about the handlers.
    dispatcher.on('logLine', vi.fn())
    dispatcher.accept(logLine(5))
    dispatcher.accept(logLine(5))
    expect(dispatcher.lastSeen).toBe(5)
    expect(dispatcher.accept(logLine(6))).toEqual({ kind: 'applied' })
  })

  it('asks for a snapshot on a gap and does not apply the event', () => {
    // ★ The interface cannot repair a gap from the events themselves — the missed ones are gone — so the only correct
    // response is to re-read. Applying the event that follows the gap would leave the picture one event out of date
    // with no way to tell.
    const { refresh, reasons } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const handler = vi.fn()
    dispatcher.on('deviceCardUpdated', handler)

    dispatcher.accept(deviceUpdated(1))
    expect(dispatcher.accept(deviceUpdated(4))).toEqual({ kind: 'stale', reason: 'gap' })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(reasons()).toEqual(['gap'])
  })

  it('asks for the snapshot once and stays known-stale until it arrives', () => {
    // ★ Two decisions, and they are different. Asking again for every event while the gap stands would be one kernel
    // call per event for as long as the interface is behind — and events keep arriving during it, so that is not a
    // brief window.
    //
    // What makes one request enough is the *flag*: `isStale` stays true until a snapshot answers, so the interface
    // knows its picture is incomplete however many events it applies in the meantime. The events after the gap are
    // applied rather than dropped, because a store that kept them is closer to right than one that did not — and the
    // refresh it is about to receive is the thing that settles it.
    const { refresh, reasons } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('logLine', vi.fn())
    dispatcher.accept(logLine(1))
    dispatcher.accept(logLine(5))
    dispatcher.accept(logLine(6))

    expect(reasons()).toEqual(['gap'])
    expect(dispatcher.isStale).toBe(true)
    expect(dispatcher.lastSeen).toBe(6)
  })

  it('marks the picture stale and clears it when a snapshot arrives', () => {
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('logLine', vi.fn())
    dispatcher.accept(logLine(1))
    expect(dispatcher.isStale).toBe(false)

    dispatcher.accept(logLine(9))
    expect(dispatcher.isStale).toBe(true)

    dispatcher.snapshotTaken()
    expect(dispatcher.isStale).toBe(false)
  })

  it('applies the first event rather than calling it a gap', () => {
    // The kernel's counter starts at one, so an interface that started mid-stream — a reload, a second window — would
    // otherwise re-read the snapshot it had just taken.
    const { refresh, reasons } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('logLine', vi.fn())
    expect(dispatcher.accept(logLine(1))).toEqual({ kind: 'applied' })
    expect(reasons()).toEqual([])
  })
})

describe('an event this build does not know', () => {
  it('asks for a snapshot rather than doing nothing', () => {
    // ★ The case an interface that predates its kernel is in. Two causes and the same response: the kernel is newer, or
    // a store failed to register. Either way the picture is incomplete, so the interface re-reads rather than
    // pretending the event did not happen — which is what a `switch` with a default of `nothing` does.
    const { refresh, reasons } = refresher()
    const dispatcher = new EventDispatcher(refresh)

    // `notice` is a real variant with no handler registered, which is the shape of "newer kernel, older interface".
    const fromANewerKernel: SequencedKernelEvent = {
      seq: 1,
      at: 1,
      type: 'notice',
      payload: { message: 'hello', params: {} }
    }
    expect(dispatcher.accept(fromANewerKernel)).toEqual({ kind: 'stale', reason: 'unknown' })
    expect(reasons()).toEqual(['unknown'])
    expect(dispatcher.unknownEvents).toBe(1)
  })

  it('counts them, because an interface that predates the kernel would otherwise log per event', () => {
    // A log line per unknown event is one per event on a busy kernel, and the number is what a diagnostic report wants
    // rather than a flood of identical lines.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    for (let seq = 1; seq <= 5; seq += 1) {
      dispatcher.accept({
        seq,
        at: seq,
        type: 'notice',
        payload: { message: 'hello', params: {} }
      })
    }
    expect(dispatcher.unknownEvents).toBe(5)
    // And it is reported, so a user filing a bug can say which build of the interface they have.
    expect(dispatcher.report().unknown).toBe(5)
  })

  it('still routes the kinds that do have a handler', () => {
    // An unknown event is a reason to re-read, not a reason to stop. A dispatcher that gave up after one would leave
    // the interface frozen from its first encounter with a newer kernel.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const handler = vi.fn()
    dispatcher.on('logLine', handler)

    dispatcher.accept({
      seq: 1,
      at: 1,
      type: 'notice',
      payload: { message: 'hello', params: {} }
    })
    dispatcher.snapshotTaken()
    expect(dispatcher.accept(logLine(2))).toEqual({ kind: 'applied' })
    expect(handler).toHaveBeenCalledTimes(1)
  })
})

describe('resetting for a reload', () => {
  it('forgets the sequence so the first event after a reload is not a gap', () => {
    // Sequence numbers are not persisted across a reload: the interface that comes back asks for a snapshot, and a
    // remembered number would make the first event after it look like a gap.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('logLine', vi.fn())
    dispatcher.accept(logLine(100))
    dispatcher.accept(logLine(200))
    expect(dispatcher.isStale).toBe(true)

    dispatcher.reset()
    expect(dispatcher.lastSeen).toBeUndefined()
    expect(dispatcher.isStale).toBe(false)
    expect(dispatcher.accept(logLine(1))).toEqual({ kind: 'applied' })
    expect(dispatcher.unknownEvents).toBe(0)
  })

  it('reports what a diagnostic bundle needs', () => {
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    dispatcher.on('logLine', vi.fn())
    dispatcher.accept(logLine(1))
    dispatcher.accept(logLine(3))
    expect(dispatcher.report()).toEqual({ last: 3, gaps: 1, unknown: 0, stale: true })
  })
})

describe('a run of events in every order', () => {
  it('never applies an event twice and never applies one out of order', () => {
    // A property rather than a case: whatever the channel delivers, the store sees each sequence at most once and
    // never sees one after the gap that followed it.
    const { refresh } = refresher()
    const dispatcher = new EventDispatcher(refresh)
    const applied: number[] = []
    dispatcher.on('logLine', event => applied.push(event.payload.line.length))

    // A stream with a gap, a repeat, and a backwards jump.
    const sequences = [1, 2, 2, 5, 4, 6, 6, 7]
    const decisions = sequences.map(seq => dispatcher.accept(logLine(seq)).kind)

    // The `2` is a repeat and the `4` is *behind* the sequence: neither advances it, so both are ignored. The `5` jumps
    // and is the one gap, so it asks for a snapshot — and the snapshot request *advances* the sequence to five, which
    // is what makes `6` and `7` contiguous again.
    //
    // That is the design rather than an accident: asking for a fresh picture is the interface deciding where it stands,
    // and an interface that treated every event after a gap as another gap would be one that could not make progress
    // without a round trip per event.
    expect(decisions).toEqual(['applied', 'applied', 'ignored', 'stale', 'ignored', 'applied', 'ignored', 'applied'])

    // The picture is announced as whole once the snapshot answers, and the stream continues.
    dispatcher.snapshotTaken()
    expect(dispatcher.isStale).toBe(false)
    expect(dispatcher.accept(logLine(8))).toEqual({ kind: 'applied' })
    // The gap event itself was never applied — it is the one the interface could not place — while the contiguous
    // events after it were, because dropping them would put the store further behind than keeping them.
    // Four applications: 1, 2, 6, 7 and 8 — the gap event (5) is the one that was never applied.\n    expect(applied).toEqual([1, 1, 1, 1, 1])
  })
})
