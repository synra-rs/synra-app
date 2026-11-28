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

import type { SynraError } from '@synra/protocol'
import { createSynraError } from '../errors'

export { asSynraError, isSynraErrorValue } from '../errors'

/** A refusal that says what is missing rather than what went wrong. */
export function unsupported(feature: string): SynraError {
  return createSynraError('unsupported', { feature })
}

/** A refusal for a capability this plugin was not granted. */
export function denied(capability: string): SynraError {
  return {
    ...createSynraError('capability_denied', { capability }),
    actions: [
      { kind: 'requestCapability', capability }
    ]
  }
}

/** A refusal for an argument the host cannot accept. */
export function invalidArgument(detail: string): SynraError {
  return createSynraError('invalid_argument', { detail })
}
