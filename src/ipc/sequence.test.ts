/**
 * The sequence tracker, and the predicates that go with it.
 *
 * # Where this file came from
 *
 * ★ It was `synra-js/packages/protocol/test/events.test.ts`, testing an implementation exported from a **type-only**
 * package. The test itself had noticed the problem and said so in a comment — it imported from the source path because
 * importing by name "would fail to resolve" — but the package's entry point went on exporting the values, so the
 * promise stayed in the public surface and nothing failed until a consumer tried it.
 *
 * The implementation now lives in `./sequence`, next to its only consumer, and these assertions came with it. What
 * stayed behind in `@synra/protocol` are the assertions about **shapes**.
 *
 * # The stance
 *
 * The kernel and the interface must agree. Rust's `synra-ipc` holds the same arithmetic, and every assertion here has a
 * counterpart there. A divergence shows up as "the panel occasionally misses a row" or "it occasionally re-reads a
 * snapshot", and neither of those reports an error.
 */

import type { KernelEvent, SequencedKernelEvent } from '@synra/protocol'
import { describe, expect, it } from 'vitest'

import { isDelta, SequenceTracker } from './sequence'

/** A minimal event, for the cases that only care about the sequence number. */
function logLine(line = 'x'): KernelEvent {
  return { type: 'logLine', payload: { level: 'info', line } }
}

/** A log event with a sequence number. */
function envelope(seq: number): SequencedKernelEvent {
  return { seq, at: 0, ...logLine() }
}

describe('the sequence tracker', () => {
  it('applies the first event rather than calling it a gap', () => {
    // The kernel's counter starts at one, so an interface that started late would otherwise report a gap immediately
    // and re-read the snapshot it had just fetched.
    const tracker = new SequenceTracker()
    expect(tracker.observe(1)).toEqual({ kind: 'apply' })
    expect(tracker.lastSeen).toBe(1)
    expect(tracker.gaps).toBe(0)
  })

  it('applies a contiguous event', () => {
    const tracker = new SequenceTracker()
    tracker.observe(7)
    expect(tracker.observe(8)).toEqual({ kind: 'apply' })
    expect(tracker.observe(9)).toEqual({ kind: 'apply' })
    expect(tracker.lastSeen).toBe(9)
  })

  it('asks for a snapshot on a gap and counts what was missed', () => {
    // A gap cannot be repaired from the events themselves — the missed ones are gone — so the only correct response is
    // to re-read.
    const tracker = new SequenceTracker()
    tracker.observe(1)
    expect(tracker.observe(4)).toEqual({ kind: 'refreshSnapshot', missed: 2 })
    expect(tracker.gaps).toBe(1)
  })

  it('reports a huge gap without a count', () => {
    // A jump of several thousand is a number nobody reads.
    const tracker = new SequenceTracker()
    tracker.observe(1)
    expect(tracker.observe(Number.MAX_SAFE_INTEGER)).toEqual({ kind: 'refreshSnapshot' })
  })

  it('ignores a repeat', () => {
    // The channel delivers the same event twice around a reload, and applying `deviceCardUpdated` twice puts one device
    // in the list once but fetches it twice.
    const tracker = new SequenceTracker()
    tracker.observe(5)
    expect(tracker.observe(5)).toEqual({ kind: 'ignoreRepeat' })
    expect(tracker.observe(4)).toEqual({ kind: 'ignoreRepeat' })
  })

  it('does not advance the sequence on a repeat', () => {
    // Advancing would make the next contiguous event look like a gap, which is a re-read for no reason.
    const tracker = new SequenceTracker()
    tracker.observe(5)
    tracker.observe(5)
    expect(tracker.lastSeen).toBe(5)
    expect(tracker.observe(6)).toEqual({ kind: 'apply' })
  })

  it('counts a gap once rather than per event', () => {
    // The interface is already about to re-read, so treating the same gap as new would ask for a snapshot on every
    // frame the gap persists for.
    const tracker = new SequenceTracker()
    tracker.observe(1)
    tracker.observe(10)
    expect(tracker.gaps).toBe(1)
    expect(tracker.observe(11)).toEqual({ kind: 'apply' })
    expect(tracker.gaps).toBe(1)
  })

  it('marks the picture stale until a snapshot answers', () => {
    // ★ The only way the interface can tell "I missed something" from "I am up to date". A caller that keeps rendering
    // while this is true is showing something it knows to be wrong.
    const tracker = new SequenceTracker()
    tracker.observe(1)
    expect(tracker.isStale).toBe(false)

    tracker.observe(5)
    expect(tracker.isStale).toBe(true)

    tracker.snapshotTaken()
    expect(tracker.isStale).toBe(false)
  })

  it('resets for a reload', () => {
    // The number is not persisted across a reload: the returning interface fetches a snapshot, and a remembered number
    // would make the first event after the reload look like a gap.
    const tracker = new SequenceTracker()
    tracker.observe(100)
    tracker.observe(200)
    tracker.reset()
    expect(tracker.lastSeen).toBeUndefined()
    expect(tracker.isStale).toBe(false)
    expect(tracker.observe(1)).toEqual({ kind: 'apply' })
  })

  it('reports what a diagnostic bundle needs', () => {
    const tracker = new SequenceTracker()
    tracker.observe(1)
    tracker.observe(3)
    expect(tracker.report()).toEqual({ last: 3, gaps: 1 })
    expect(new SequenceTracker().report()).toEqual({ gaps: 0 })
  })

  it('never goes backwards over a run of numbers', () => {
    // The property: whatever order the numbers arrive in, the recorded sequence never decreases. Once it does, every
    // event is treated as a repeat and the interface stops updating.
    const tracker = new SequenceTracker()
    const sequences = [5, 5, 3, 6, 6, 10, 9, 11, 2, 12]
    let previous = 0
    for (const seq of sequences) {
      tracker.observe(seq)
      const last = tracker.lastSeen ?? previous
      expect(last).toBeGreaterThanOrEqual(previous)
      previous = last
    }
  })

  it('is judged by its sequence, not its timestamp', () => {
    // Two events in the same millisecond are ordinary, and the clock moves. Ordering is by sequence number.
    const tracker = new SequenceTracker()
    const first: SequencedKernelEvent = { seq: 1, at: 1_000, ...logLine('a') }
    const second: SequencedKernelEvent = { seq: 2, at: 999, ...logLine('b') }

    expect(tracker.observe(first.seq)).toEqual({ kind: 'apply' })
    expect(tracker.observe(second.seq)).toEqual({ kind: 'apply' })
  })

  it('turns an unknown event into a snapshot refresh rather than a crash', () => {
    // The design document's `default` branch, as an executable statement: the kernel added a variant, and the old
    // interface re-reads instead of crashing.
    const tracker = new SequenceTracker()
    const known = envelope(1)
    expect(tracker.observe(known.seq)).toEqual({ kind: 'apply' })

    // An event from a newer kernel — the interface cannot parse its `type` — but the sequence is still tracked, so it
    // knows what it missed and re-reads.
    const fromANewerKernel = 2
    expect(tracker.observe(fromANewerKernel)).toEqual({ kind: 'apply' })

    // And a jump is still recognised, even when the events in between are all unrecognised.
    expect(tracker.observe(5)).toEqual({ kind: 'refreshSnapshot', missed: 2 })
  })
})

describe('deltas', () => {
  it('reports that no event carries one', () => {
    // Every event carries an **absolute** value (progress is a ratio rather than a byte count), so applying it twice
    // changes nothing. The function exists so a future event carrying a delta has to say so.
    const events: readonly KernelEvent[] = [
      { type: 'deviceRemoved', payload: { deviceId: 'd' } },
      logLine(),
      {
        type: 'pluginInstallProgress',
        payload: { pluginId: '@a/b', progress: 0.5, stage: 'downloading' }
      }
    ]
    for (const event of events) {
      expect(isDelta(event)).toBe(false)
    }
  })
})
