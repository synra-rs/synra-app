/**
 * The refusals this facade hands a plugin, and the test that recognises one.
 *
 * # The rule this file is built around
 *
 * **Every method exists, and a method the host cannot honour refuses rather than fakes.** The surface library says this
 * out loud for `device.pick`, and the reason generalises: an author who calls a method that is missing gets
 * `TypeError: not a function`, which names neither the host nor the plugin and cannot be handled. An author who calls one
 * that exists and refuses gets a `SynraError` with a code, which is an ordinary branch.
 *
 * The two refusal constructors live here rather than beside the namespace that throws them, because every namespace
 * throws them and a refusal that read differently in each group would be a second vocabulary for one thing.
 */

import type { ErrorCode, SynraError } from '@synra/protocol'

import type { IpcError } from '../client'

/** A refusal that says what is missing rather than what went wrong. */
export function unsupported(feature: string): SynraError {
  return {
    code: 'unsupported' as ErrorCode,
    message: `error.unsupported`,
    retryable: false,
    details: { feature }
  } as unknown as SynraError
}

/** A refusal for a capability this plugin was not granted. */
export function denied(capability: string): SynraError {
  return {
    code: 'capability_denied' as ErrorCode,
    message: `error.capabilityDenied`,
    retryable: false,
    details: { capability }
  } as unknown as SynraError
}

/** Turns whatever the bridge rejected with into the shape a plugin can branch on. */
export function asSynraError(reason: unknown): SynraError {
  const candidate = reason as Partial<IpcError> | undefined
  if (candidate !== undefined && typeof candidate.code === 'string')
    return candidate as unknown as SynraError
  return unsupported('unknown')
}

/**
 * Whether a value is a `SynraError`.
 *
 * ★ A named function, not only the method.
 *
 * `isCode` has to branch on the same test. Written as `isSynraError(value)` inside the object literal it would resolve
 * to the property being defined a line above, which is a self-reference through `synra` — so this is the function the
 * method and its sibling both call.
 */
export function isSynraErrorValue(value: unknown): value is SynraError {
  const candidate = value as Partial<SynraError> | undefined
  return candidate !== undefined
    && typeof candidate === 'object'
    && typeof candidate.code === 'string'
    && typeof candidate.message === 'string'
}
