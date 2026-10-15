/**
 * Tracking the kernel's event sequence, so the interface can tell a gap from a repeat.
 *
 * # Why this is here and not in `@synra/protocol`
 *
 * ★ It used to be there, exported from that package's entry point — and that was a promise the package could not keep.
 * `@synra/protocol` is a **type-only** package by design ([25 §2.1](../../../design-docs/25-js-monorepo.md)): its
 * `package.json` exposes `types` and nothing else, and the build emits no `.js`. So
 * `import { SequenceTracker } from '@synra/protocol'` **type-checked and could not resolve at runtime** — the consumer
 * got `undefined` or a bundler error, with nothing warning them first.
 *
 * The package's own test had noticed, writing that importing these by name "would fail to resolve" and importing from
 * the source path instead. But the entry point went on exporting them, so the promise stayed in the public surface.
 *
 * The host, unlike a plugin author's UI, genuinely has runtime — it is an application, not a types package. So the
 * arithmetic lives with its only consumer. What stays in `@synra/protocol` is the *shape*: `EventVerdict` and
 * `SequenceReport`, which this file implements against.
 *
 * # The rule this file exists for
 *
 * Every event is judged by its sequence number **before** it is applied, and the judgement has three outcomes: apply
 * it, ignore it as a repeat, or ask for a fresh snapshot because something was missed. The interface cannot repair a
 * gap from the events themselves — the missed ones are gone — so the only correct response is to re-read.
 *
 * The Rust side (`synra-ipc`) holds the same arithmetic, and the two must agree: a divergence shows up as "the panel
 * occasionally misses a row" or "it occasionally re-reads a snapshot", and neither of those reports an error.
 */

import type { EventVerdict, SequenceReport } from '@synra/protocol'

/** The largest gap worth counting. */
export const MAX_COUNTED_GAP = 1000

/**
 * Tracks the sequence number, so the interface can tell a gap from a repeat.
 *
 * Holds the last number seen and nothing else — no I/O, no kernel, no clock. The caller feeds it every envelope and
 * acts on the verdict, which is what makes the arithmetic testable on its own.
 */
export class SequenceTracker {
  private last: number | undefined
  private gapCount = 0
  private stale = false

  /** The last sequence number applied. */
  get lastSeen(): number | undefined {
    return this.last
  }

  /** How many gaps have been reported. */
  get gaps(): number {
    return this.gapCount
  }

  /**
   * Whether the picture is known to be untrustworthy.
   *
   * True from the moment a gap is reported until a snapshot answers it. A caller that keeps rendering while this is
   * true is showing something it knows to be wrong.
   */
  get isStale(): boolean {
    return this.stale
  }

  /**
   * Judges one event and records it.
   *
   * ★ **The first event is not a gap.** With nothing seen yet, any number is accepted: the kernel's counter starts at
   * one, so an interface that started late would otherwise report a gap immediately and re-read the snapshot it had
   * just fetched.
   */
  observe(seq: number): EventVerdict {
    const last = this.last

    if (last === undefined) {
      this.last = seq
      return { kind: 'apply' }
    }
    if (seq <= last) {
      // A repeat. **The number is not advanced** — advancing would make the next contiguous event look like a gap.
      return { kind: 'ignoreRepeat' }
    }
    if (seq === last + 1) {
      this.last = seq
      return { kind: 'apply' }
    }

    // A gap. Recorded, and the picture marked untrustworthy.
    const missed = seq - (last + 1)
    this.last = seq
    this.gapCount += 1
    this.stale = true
    // A jump of several thousand would be a number nobody reads, and subtracting from an externally-supplied value can
    // overflow — so the count is reported only when it is one worth counting.
    return missed <= MAX_COUNTED_GAP ? { kind: 'refreshSnapshot', missed } : { kind: 'refreshSnapshot' }
  }

  /** Records that a snapshot was taken, so the picture is trustworthy again. */
  snapshotTaken(): void {
    this.stale = false
  }

  /**
   * Forgets everything, for when the interface is reloaded.
   *
   * The sequence number is **not** persisted across a reload: the returning interface fetches a snapshot, and a
   * remembered number would make the first event after the reload look like a gap.
   */
  reset(): void {
    this.last = undefined
    this.stale = false
  }

  /** A snapshot for the diagnostics bundle. */
  report(): SequenceReport {
    return this.last === undefined
      ? { gaps: this.gapCount }
      : { last: this.last, gaps: this.gapCount }
  }
}

/**
 * Whether an event carries a delta.
 *
 * Every event is `false` today: each carries an **absolute** value (progress is a ratio rather than a byte count), so
 * applying it twice changes nothing. The function exists so that a future event carrying a delta has to say so rather
 * than being assumed idempotent.
 */
export function isDelta(_event: { readonly type: string }): boolean {
  return false
}
