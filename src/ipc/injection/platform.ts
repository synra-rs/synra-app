/**
 * The platform and clipboard namespaces, over the plugin channel.
 *
 * # Why the grant is checked here *and* in the kernel
 *
 * The check here is the one a plugin author sees: a missing capability is a `capability_denied` without a round trip, so
 * a `try`/`catch` around `platform.notify` behaves the same whether the page is live or the host is unreachable. The
 * kernel's check is the one that matters — a page can call `plugin_ui_rpc` directly, and the session's `CapabilitySet`
 * is the only authority on what it may do. Two checks, one answer; neither replaces the other.
 *
 * # Why the local names are the capability names, not the namespace names
 *
 * Because they are different vocabularies on purpose: `clipboard.read` is a method a page calls, and
 * `platform:read-clipboard` is the permission the user granted. The first version of this file checked
 * `clipboard_read` — a third spelling that appears nowhere — so every call was denied with a capability the manifest
 * could not even have declared.
 */

import type { ClipboardNamespace, PlatformNamespace } from '@synra/client'

import type { InjectionContext } from './context'
import { asSynraError, denied } from './errors'

/** A namespace that has the context's token and bridge. */
function rpc(context: InjectionContext) {
  return <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>
}

/** The user's browser and notifications. */
export function createPlatformNamespace(context: InjectionContext): PlatformNamespace {
  const granted = new Set(context.granted)
  const call = rpc(context)

  return {
    openUrl: async (url: string): Promise<void> => {
      if (!granted.has('platform:open-url'))
        throw denied('platform:open-url')
      await call<null>('platform.openUrl', { url })
    },
    notify: async (request: { readonly title: string, readonly body: string }): Promise<void> => {
      if (!granted.has('platform:notify'))
        throw denied('platform:notify')
      await call<null>('platform.notify', { title: request.title, body: request.body })
    }
  }
}

/** The system clipboard. */
export function createClipboardNamespace(context: InjectionContext): ClipboardNamespace {
  const granted = new Set(context.granted)
  const call = rpc(context)

  return {
    read: async (): Promise<string> => {
      if (!granted.has('platform:read-clipboard'))
        throw denied('platform:read-clipboard')
      // ★ `null` is an empty clipboard, not a refusal. The contract says this method answers a string, and turning
      // "nothing is copied" into an error would make the ordinary case a `catch` for every caller.
      const text = await call<string | null>('clipboard.read')
      return text ?? ''
    },
    write: async (text: string): Promise<void> => {
      if (!granted.has('platform:write-clipboard'))
        throw denied('platform:write-clipboard')
      await call<null>('clipboard.write', { text })
    }
  }
}
