/**
 * Where kernel events arrive, and what the interface does with each one.
 *
 * # The one rule this file exists for
 *
 * Every event is **judged by its sequence number before it is applied**, and the judgement has three outcomes: apply
 * it, ignore it as a repeat, or ask for a fresh snapshot because something was missed.
 *
 * The interface cannot repair a gap from the events themselves — the missed ones are gone — so the only correct
 * response is to re-read. What the sequence number buys is that the interface *knows* to re-read, rather than showing
 * a device list that is quietly an event out of date. That failure is invisible: the panel renders, the numbers are
 * plausible, and one of them is wrong.
 *
 * # Why the judgement is here rather than in each store
 *
 * Six stores consume events, and each one applying its own event without knowing whether it arrived in order would be
 * six chances to get the sequence handling wrong — and the symptom would be "sometimes the list is stale", which is
 * the hardest kind of report to act on. So the dispatcher owns the sequence and routes only what it has decided is
 * worth applying.
 *
 * # Why the handlers are registered rather than cased
 *
 * A `switch` over the event's `type` would put every store's logic in one function, and a variant added to the kernel
 * would be a new arm in a file that already has seventeen. A handler per event kind means a store subscribes to what it
 * cares about, and the dispatcher's `default` is where an event this build does not know lands — an interface that
 * predates the kernel it is talking to, which must re-read rather than crash.
 */

import { SequenceTracker, isDelta } from '@synra/protocol'
import type { KernelEvent, SequencedKernelEvent } from '@synra/protocol'

/**
 * What the dispatcher decided about one event.
 *
 * Returned rather than acted on, so a test asserts the decision and the caller decides what to do about it — a
 * `refreshSnapshot` has to ask the kernel for one, which is I/O.
 */
export type Verdict =
  | { readonly kind: 'applied' }
  | { readonly kind: 'ignored' }
  | { readonly kind: 'stale'; readonly reason: 'gap' | 'unknown' }

/**
 * What happens when the interface needs a fresh picture.
 *
 * A function rather than an event, because re-reading is a *kernel call* and the dispatcher does not make calls. The
 * caller passes something that does, and a test passes something that records.
 */
export type RefreshSnapshot = (reason: 'gap' | 'first' | 'unknown') => void

/** A handler for one event kind. */
export type Handler<E extends KernelEvent = KernelEvent> = (event: E) => void

/**
 * Routes kernel events to the stores that care about them.
 *
 * # One dispatcher per interface
 *
 * Not one per store. The sequence number is a property of the *channel*, so two trackers would each see half the
 * events and both conclude they had missed some — and the interface would re-read two snapshots for every event.
 */
export class EventDispatcher {
  readonly #tracker: SequenceTracker
  readonly #handlers: Map<KernelEvent['type'], Handler<never>[]>
  readonly #refresh: RefreshSnapshot
  #unknown = 0

  /**
   * @param refresh - called when the interface has to re-read. Called **after** the verdict is decided, so a test can
   *   assert the order of "noticed a gap" and "asked for a snapshot".
   */
  constructor(refresh: RefreshSnapshot) {
    this.#tracker = new SequenceTracker()
    this.#handlers = new Map()
    this.#refresh = refresh
  }

  /**
   * Subscribes to one event kind.
   *
   * @returns a function that unsubscribes, which is what a component's cleanup calls. Returning it rather than
   *   requiring the caller to hold a reference is what makes the subscription impossible to leak by forgetting.
   */
  on<T extends KernelEvent['type']>(
    type: T,
    handler: Handler<Extract<KernelEvent, { type: T }>>,
  ): () => void {
    const existing = this.#handlers.get(type) ?? []
    // The cast is the price of one map instead of one field per kind. The key is the discriminant of the value, so a
    // handler registered for `deviceCardUpdated` can only be called with one.
    existing.push(handler as Handler<never>)
    this.#handlers.set(type, existing)

    return () => {
      const current = this.#handlers.get(type)
      if (!current) return
      const at = current.indexOf(handler as Handler<never>)
      if (at >= 0) current.splice(at, 1)
      if (current.length === 0) this.#handlers.delete(type)
    }
  }

  /**
   * Takes an event from the kernel.
   *
   * # The order of the two decisions
   *
   * The sequence is judged first and the event's *kind* second, because a gap makes the kind irrelevant: an event
   * whose predecessor was missed is one the interface cannot place, whatever it carries.
   */
  accept(envelope: SequencedKernelEvent): Verdict {
    const judgement = this.#tracker.observe(envelope.seq)
    if (judgement.kind === 'ignoreRepeat') {
      // A repeat is not a gap: the channel delivered the same event twice around a reload, and applying it twice is
      // what makes a device appear once and be fetched twice.
      return { kind: 'ignored' }
    }
    if (judgement.kind === 'refreshSnapshot') {
      this.#refresh('gap')
      return { kind: 'stale', reason: 'gap' }
    }

    // Destructured rather than passed through, because `seq` and `at` are the envelope's and the handler wants the
    // event. A handler that saw the sequence would be one that could make a second judgement about it.
    const { seq: _seq, at: _at, ...event } = envelope
    const handlers = this.#handlers.get(event.type)
    if (!handlers) {
      // ★ An event kind this build does not handle. Two causes and the same response: the kernel is newer than the
      // interface, or a store failed to register. Either way the picture is incomplete, so the interface re-reads
      // rather than pretending the event did not happen.
      //
      // Counted rather than logged: an interface that predates the kernel would log once per event, and the number is
      // what a diagnostic report wants.
      this.#unknown += 1
      this.#refresh('unknown')
      return { kind: 'stale', reason: 'unknown' }
    }

    for (const handler of handlers) {
      ;(handler as Handler)(event as KernelEvent)
    }
    return { kind: 'applied' }
  }

  /**
   * Records that a fresh snapshot arrived.
   *
   * The interface's picture is whole again, and the "I may be stale" flag clears. Called by whatever fetched the
   * snapshot, *after* it has applied it — calling it before would mark the old picture as current.
   */
  snapshotTaken(): void {
    this.#tracker.snapshotTaken()
  }

  /** Whether the interface's picture may be missing something. */
  get isStale(): boolean {
    return this.#tracker.isStale
  }

  /** The last sequence number seen. */
  get lastSeen(): number | undefined {
    return this.#tracker.lastSeen
  }

  /** How many times the interface noticed a gap. */
  get gaps(): number {
    return this.#tracker.gaps
  }

  /** How many events arrived that this build has no handler for. */
  get unknownEvents(): number {
    return this.#unknown
  }

  /**
   * Drops everything, for a reload.
   *
   * Sequence numbers are not persisted across a reload: the interface that comes back asks for a snapshot, and a
   * remembered number would make the first event after it look like a gap.
   */
  reset(): void {
    this.#tracker.reset()
    this.#unknown = 0
  }

  /** What a diagnostic report wants to know. */
  report(): { last?: number; gaps: number; unknown: number; stale: boolean } {
    const base = this.#tracker.report()
    return { ...base, unknown: this.#unknown, stale: this.isStale }
  }
}

/**
 * Whether applying an event twice would change anything.
 *
 * Re-exported from the protocol rather than re-implemented: the answer is a property of the *event's shape*, and the
 * kernel's side of that decision is in the same file the type is.
 */
export { isDelta }
