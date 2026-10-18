/**
 * The page's own storage, over the plugin channel.
 *
 * # What is stored
 *
 * **A value is JSON text**, because the headless half of the same plugin shares this table and its Wasm side reads a
 * string. `set('theme', 'dark')` therefore stores `"dark"` and `get` reads it back as the string `dark`; an object goes
 * in and comes out as an object. A value written by a half that did not encode it — a raw `dark` — is returned as the
 * raw string rather than throwing, because the alternative is a `SyntaxError` from somebody else's write appearing as a
 * plugin bug.
 *
 * # Why every call goes through `plugin_ui_rpc`
 *
 * Because the grants are checked in the kernel against the session, and `storage`-table access from a page that was
 * never granted `storage:local:read/write` is exactly what the capability model exists to refuse. The namespace does not
 * decide that; it just carries the token.
 *
 * # Why the method names are spelled here
 *
 * They are the wire names from `PluginUiOp`, and they are the only part of this file that could drift from the kernel.
 * A test asserts the call each method makes, so a rename in one place and not the other is a red test rather than a
 * runtime `unsupported`.
 */

import type { StorageNamespace } from '@synra/client'

import type { InjectionContext } from './context'
import { asSynraError, invalidArgument } from './errors'

/** The storage namespace, over the plugin channel. */
export function createStorageNamespace(context: InjectionContext): StorageNamespace {
  const call = <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>

  function get<T = unknown>(key: string): Promise<T | undefined>
  function get<T>(key: string, defaults: { fallback: T }): Promise<T>
  async function get<T>(key: string, defaults?: { fallback: T }): Promise<T | undefined> {
    const stored = await call<string | null>('storage.get', { key })
    if (stored === null) {
      return defaults?.fallback
    }
    try {
      return JSON.parse(stored) as T
    }
    catch {
      // ★ A raw value, from a writer that did not JSON-encode. Returning it is the only answer that does not turn one
      // half's storage into an exception in the other half.
      return stored as T
    }
  }

  return {
    get,
    set: async <T = unknown>(key: string, value: T): Promise<void> => {
      let encoded: string | undefined
      try {
        encoded = JSON.stringify(value)
      }
      catch (cause) {
        throw invalidArgument(`the value for \`${key}\` cannot be serialised: ${String(cause)}`)
      }
      if (encoded === undefined) {
        // `undefined`, a function or a symbol: JSON has no representation for it, and storing the text `undefined` would
        // read back as a string that nobody wrote.
        throw invalidArgument(`the value for \`${key}\` has no JSON representation`)
      }
      await call<null>('storage.set', { key, value: encoded })
    },
    delete: async (key: string): Promise<void> => {
      await call<null>('storage.delete', { key })
    },
    keys: (prefix?: string): Promise<string[]> =>
      call<string[]>('storage.keys', prefix === undefined ? {} : { prefix }),
    quota: (): Promise<{ usedBytes: number, maxBytes: number }> =>
      call<{ usedBytes: number, maxBytes: number }>('storage.quota')
  }
}
