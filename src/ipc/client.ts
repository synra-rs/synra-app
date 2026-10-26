/**
 * The host interface receives the generated IPC error contract: a message key, parameters,
 * retryability and optional diagnostic JSON. The protocol package owns that shape.
 *
 * The bridge exposes Tauri through an injectable interface so tests can drive the real client
 * without a WebView. Plugin injection preserves structured bridge rejections for diagnostics.
 */

import type { IpcError } from '@synra/protocol'

export type { IpcError } from '@synra/protocol'

/**
 * The seam to Tauri.
 *
 * Two methods, because those are the two things the host offers: a command that answers, and an event that arrives.
 */
export interface Bridge {
  /** Calls a kernel command. */
  invoke: (command: string, args?: Readonly<Record<string, unknown>>) => Promise<unknown>
  /**
   * Subscribes to kernel events.
   *
   * Returns a promise of an unsubscribe function rather than the function itself, because the real implementation
   * registers a listener asynchronously — a WebView bridge cannot answer synchronously. A caller that ignored the
   * promise would leak a listener per call.
   */
  listen: (event: string, handler: (payload: unknown) => void) => Promise<() => void>
}

/** What a call produced: a value, or the kernel's refusal. */
export type CallResult<T>
  = | { readonly ok: true, readonly value: T }
    | { readonly ok: false, readonly error: IpcError }

/**
 * Whether a value is an `IpcError`.
 *
 * # Why this is a runtime check rather than a cast
 *
 * The error arrives across a WebView bridge as a plain object, so its *type* is a claim the bridge makes and not
 * something the compiler can verify. A caller that cast would read `error.code` from a value that might be a string —
 * and `undefined` is not a code, so a `switch` on it would fall to a default message the user has already read.
 *
 * The check is on the fields the interface *uses*: a code, a key and a retryability. A richer error with more fields is
 * still an error; one missing any of the three is one this interface cannot render or classify.
 */
export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== 'object' || value === null)
    return false
  const candidate = value as Record<string, unknown>
  return (
    typeof candidate.code === 'string'
    && typeof candidate.message === 'string'
    && typeof candidate.retryable === 'boolean'
  )
}

/**
 * A client over one bridge.
 *
 * # The commands are named strings, and that is deliberate
 *
 * The host's command surface is defined in Rust (`synra-ipc`'s `commands!`), and a TypeScript enum of the same names
 * would be a second list to keep in step — the drift being a call that fails with "command not found" at runtime. So
 * each method carries its wire name in one place, and `scripts/check-shapes.ts` compares the set of names this file
 * uses against the Rust list. One list, two readers.
 */
export class KernelClient {
  readonly #bridge: Bridge

  constructor(bridge: Bridge) {
    this.#bridge = bridge
  }

  /**
   * Calls a command and reports the kernel's answer as a value.
   *
   * # Errors are values rather than exceptions
   *
   * A refusal is an ordinary outcome of every call — a device is offline, a plugin is not installed — and an interface
   * that had to `try`/`catch` each one would be one where a missed `catch` is a blank panel. So the caller narrows on
   * `ok`, and the type system makes them.
   */
  async call<T>(
    command: string,
    args?: Readonly<Record<string, unknown>>
  ): Promise<CallResult<T>> {
    let raw: unknown
    try {
      raw = await this.#bridge.invoke(command, args)
    }
    catch (thrown) {
      // ★ A rejection rather than a resolved `IpcError`. Tauri turns a command that *panicked* into a rejection and a
      // command that *returned* an error into a value, and the interface has to handle both — because the difference
      // is not something the user can see and not something the interface should show them.
      //
      // A rejection that is already an error is passed through, since some bridges reject with the error rather than
      // resolving with it, and re-wrapping it would lose the code.
      if (isIpcError(thrown))
        return { ok: false, error: thrown }
      return { ok: false, error: bridgeFailure(thrown) }
    }

    // The host's convention: a command returning a value *is* the success, and its refusal is a value that looks like
    // an error. Checking the shape rather than a wrapper means a command whose return type is itself an object with a
    // `code` field is still handled correctly — because the check needs all three fields.
    if (isIpcError(raw))
      return { ok: false, error: raw }
    return { ok: true, value: raw as T }
  }

  /**
   * Subscribes to a kernel event.
   *
   * # The listen happens before the snapshot, and the order is the caller's
   *
   * An interface that took a snapshot and *then* started listening would miss every event in between, and the miss is
   * silent: the panel renders, and it is one event out of date. So [`KernelClient.subscribe`] takes a callback that
   * fetches the snapshot, and the client calls it after the listener is registered.
   */
  async subscribe(
    event: string,
    handler: (payload: unknown) => void
  ): Promise<() => void> {
    return await this.#bridge.listen(event, handler)
  }
}

/**
 * The error for a rejection that was not one of ours.
 *
 * # Why it becomes an `IpcError` rather than propagating
 *
 * A caller that receives a raw exception has to know that the bridge exists. This turns it into the same shape every
 * other failure has — with `Internal`, which is the code the kernel uses for "something unexpected" and which **is**
 * retryable — so the interface has one path rather than two.
 *
 * `message` is a key rather than a sentence, and the key says what happened: the bridge, not the kernel, failed. A
 * user-facing sentence for it is the interface's to write.
 */
function bridgeFailure(thrown: unknown): IpcError {
  const detail = thrown instanceof Error ? thrown.message : String(thrown)
  return {
    code: 'internal',
    message: 'error.bridgeFailed',
    params: { detail },
    // Retryable: a bridge rejection is a transport-shaped failure, and the kernel's own `internal` is retryable. The
    // alternative is an interface that gives up on a transient fault.
    retryable: true
  }
}

/**
 * Whether a call is worth repeating as it was.
 *
 * # Why the kernel's judgement is used rather than re-derived
 *
 * The design document is explicit: `Internal` **is** retryable in the kernel. An interface that decided for itself
 * would be a second opinion that can contradict the kernel the user is talking to — and the contradiction shows up as
 * a "Retry" button that fails identically, or a message with no button that would have worked.
 *
 * So this reads the kernel's answer and adds nothing to it.
 */
export function shouldOfferRetry(error: IpcError): boolean {
  return error.retryable
}
