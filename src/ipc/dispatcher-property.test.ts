/**
 * A stream of events in every order, asserted as a property.
 *
 * # What is being tested, and why it is not the tracker
 *
 * `@synra/protocol`'s `SequenceTracker` has its own tests. What this file asserts is the *dispatcher's* use of it, over
 * a stream rather than one event at a time: whatever the channel delivers, the store sees each sequence at most once and
 * never sees one after the gap that followed it.
 *
 * It is apart from `dispatcher-sequence.test.ts` because it is a different *kind* of test — one stream with a gap, a
 * repeat and a backwards jump, rather than one decision per case. The factories are in `dispatcher-test-helpers.ts`,
 * which the runner does not collect.
 */

import { describe, expect, it } from 'vitest'
import { EventDispatcher } from './dispatcher'

import { logLine, refresher } from './dispatcher-test-helpers'

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
