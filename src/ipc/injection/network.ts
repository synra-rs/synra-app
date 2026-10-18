/**
 * Outbound requests, over the plugin channel.
 *
 * # Why the response is built here rather than returned as the host sent it
 *
 * Because the kernel answers with `{ status, statusText, headers, url, redirected, body }` and the contract promises a
 * **`SynraResponse`**: three mutually-exclusive readers, `ok` derived from the status, and a rejection on a second read.
 * That translation is the façade's job — the host has no business deciding what a page's second `text()` call does.
 *
 * # What is refused before the call
 *
 * A byte body and a `timeoutMs`, because neither can be honoured through this channel: the wire request carries a
 * string, and the timeout is the client's rather than the caller's. Refusing here names the feature; forwarding a value
 * the kernel would ignore would let an author ship a bound that does nothing.
 */

import type { NetworkNamespace, SynraFetchInit, SynraResponse } from '@synra/client'

import type { InjectionContext } from './context'
import { asSynraError, invalidArgument, unsupported } from './errors'

/** What the kernel answers with, before it becomes a `SynraResponse`. */
interface RawResponse {
  readonly status: number
  readonly statusText: string
  readonly headers: Readonly<Record<string, string>>
  readonly url: string
  readonly redirected: boolean
  readonly body: string
}

/** Turns the kernel's answer into the contract's response, with one read per body. */
function response(raw: RawResponse): SynraResponse {
  let read = false
  const consume = (): void => {
    if (read)
      throw invalidArgument('the response body has already been read')
    read = true
  }

  return {
    status: raw.status,
    statusText: raw.statusText,
    headers: raw.headers,
    url: raw.url,
    // ★ Derived rather than sent: the kernel reports what it received, and "2xx" is the contract's reading of that — a
    // second copy of the rule on the other side of the wire is a second thing to keep in step.
    ok: raw.status >= 200 && raw.status < 300,
    redirected: raw.redirected,
    text: async (): Promise<string> => {
      consume()
      return raw.body
    },
    json: async (): Promise<unknown> => {
      consume()
      try {
        return JSON.parse(raw.body)
      }
      catch (cause) {
        throw invalidArgument(`the body is not JSON: ${String(cause)}`)
      }
    },
    bytes: async (): Promise<Uint8Array> => {
      consume()
      // The host decoded the body as text; the bytes are its UTF-8 encoding. A response that was not text would have
      // been refused by the host's reader rather than arriving as mojibake.
      return new TextEncoder().encode(raw.body)
    }
  }
}

/** The network namespace, over the plugin channel. */
export function createNetworkNamespace(context: InjectionContext): NetworkNamespace {
  return {
    fetch: async (input: string, init: SynraFetchInit = {}): Promise<SynraResponse> => {
      if (init.timeoutMs !== undefined)
        throw unsupported('network.fetch timeoutMs')
      let body: string | undefined
      if (init.body !== undefined) {
        if (typeof init.body !== 'string')
          throw unsupported('network.fetch body')
        body = init.body
      }

      const raw = await context.bridge
        .invoke('plugin_ui_rpc', {
          token: context.token,
          method: 'network.fetch',
          args: { input, init: { method: init.method, headers: init.headers, body } }
        })
        .catch((reason: unknown) => {
          throw asSynraError(reason)
        }) as RawResponse

      return response(raw)
    }
  }
}
