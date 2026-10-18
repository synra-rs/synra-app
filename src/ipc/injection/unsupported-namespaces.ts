/**
 * The namespace this build cannot answer: transfer.
 *
 * # Why it is here rather than absent
 *
 * It needs a kernel operation that does not exist in this build, and it refuses with `unsupported` rather than returning
 * an empty list, a fake id or a resolved promise. That arrives with the operation.
 *
 * The method exists so a missing one is never `TypeError: not a function`, and the refusal names the feature.
 */

import type { TransferNamespace, TransferProgress } from '@synra/client'
import type { TransferId } from '@synra/protocol'

import { unsupported } from './errors'

/** The transfer namespace, whose unsubscribe is real even though no transfer ever starts. */
export function createTransferNamespace(): TransferNamespace {
  return {
    send: async () => {
      throw unsupported('transfer.send')
    },
    status: async () => {
      throw unsupported('transfer.status')
    },
    // ★ `id` is part of the signature, so it is named even though nothing uses it yet.
    //
    // The surface is `onProgress(id, handler)`. An implementation taking only a handler would typecheck against a
    // one-argument signature and then never call anything — the shape that compiles and silently does nothing.
    onProgress: (_id: TransferId, _handler: (progress: TransferProgress) => void) => () => {
      // A no-op unsubscribe, returned for the guarantee above.
    }
  }
}
