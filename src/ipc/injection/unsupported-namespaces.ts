/**
 * The namespaces this build cannot answer: network, transfer, platform, clipboard and self.
 *
 * # Why each of them is here rather than absent
 *
 * Each needs a kernel operation that does not exist in this build, and each refuses with `unsupported` rather than
 * returning an empty list, a fake id or a resolved promise. Those arrive with the operations.
 *
 * They are one file because they are one decision — the method exists so a missing one is never `TypeError: not a
 * function`, and the refusal names the feature. The two that also check a grant say `capability_denied` instead, because
 * a denied capability is a question for the user and an unsupported one is a question for the host.
 */

import type {
  ActionDescriptor,
  ClipboardNamespace,
  NetworkNamespace,
  PlatformNamespace,
  SelfNamespace,
  TransferNamespace,
  TransferProgress
} from '@synra/client'
import type { TransferId } from '@synra/protocol'

import { denied, unsupported } from './errors'

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

/** The platform namespace: opening a URL and notifying both need kernel commands this build lacks. */
export function createPlatformNamespace(): PlatformNamespace {
  return {
    openUrl: async () => {
      throw unsupported('platform.openUrl')
    },
    notify: async () => {
      throw unsupported('platform.notify')
    }
  }
}

/** The clipboard namespace, which names the missing grant before it names the missing command. */
export function createClipboardNamespace(granted: ReadonlySet<string>): ClipboardNamespace {
  return {
    read: async () => {
      if (!granted.has('clipboard_read'))
        throw denied('clipboard_read')
      throw unsupported('clipboard.read')
    },
    write: async () => {
      if (!granted.has('clipboard_write'))
        throw denied('clipboard_write')
      throw unsupported('clipboard.write')
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
