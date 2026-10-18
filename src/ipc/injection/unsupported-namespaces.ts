/**
 * The namespaces this build cannot answer: network, transfer and self.
 *
 * # Why each of them is here rather than absent
 *
 * Each needs a kernel operation that does not exist in this build, and each refuses with `unsupported` rather than
 * returning an empty list, a fake id or a resolved promise. Those arrive with the operations.
 *
 * They are one file because they are one decision — the method exists so a missing one is never `TypeError: not a
 * function`, and the refusal names the feature.
 */

import type {
  ActionDescriptor,
  NetworkNamespace,
  SelfNamespace,
  TransferNamespace,
  TransferProgress
} from '@synra/client'
import type { TransferId } from '@synra/protocol'

import { unsupported } from './errors'

/** The network namespace, which refuses rather than reaching around the page's own CSP. */
export function createNetworkNamespace(): NetworkNamespace {
  return {
    // ★ A refusal rather than a fetch.
    //
    // The plugin page's CSP is `connect-src 'none'`, so a direct `fetch` fails in a way that looks like a network
    // problem. Routing through the kernel is the only sanctioned path, and this build has no command for it — so this
    // says so instead of reaching around the policy.
    fetch: async () => {
      throw unsupported('network.fetch')
    }
  }
}

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

/** The self namespace: the plugin's own actions and its self-invoke. */
export function createSelfNamespace(): SelfNamespace {
  return {
    invoke: async () => {
      throw unsupported('self.invoke')
    },
    actions: async (): Promise<ActionDescriptor[]> => {
      throw unsupported('self.actions')
    }
  }
}
