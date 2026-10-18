/**
 * The storage namespace, over the plugin channel.
 *
 * # What is asserted
 *
 * That every method goes through `plugin_ui_rpc` with the token the page was issued, that the wire name is the one
 * `PluginUiOp` declares, and that JSON is the representation — including the one case where a value was written by the
 * headless half as a raw string.
 */

import { describe, expect, it, vi } from 'vitest'

import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

/** A bridge whose answer is chosen by the method the call names. */
function answering(answer: (method: string, args: Readonly<Record<string, unknown>>) => unknown) {
  return vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      const method = payload?.method as string
      const args = (payload?.args ?? {}) as Readonly<Record<string, unknown>>
      return answer(method, args)
    }
  )
}

describe('storage', () => {
  it('reads a JSON value back as the value it was', async () => {
    const invoke = answering(() => '{"a":1,"b":"x"}')
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.storage.get('config')).resolves.toEqual({ a: 1, b: 'x' })
    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'storage.get',
      args: { key: 'config' }
    })
  })

  it('answers a fallback for a key that was never written', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(() => null) }) }))

    await expect(synra.storage.get('missing', { fallback: 7 })).resolves.toBe(7)
    await expect(synra.storage.get('missing')).resolves.toBeUndefined()
  })

  it('returns a raw value written by the headless half', async () => {
    // ★ The other half of this plugin writes strings through the Wasm host, which does not JSON-encode. Throwing a
    // `SyntaxError` at the page would make one half's storage an exception in the other; the raw string is the answer.
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(() => 'dark') }) }))

    await expect(synra.storage.get('theme')).resolves.toBe('dark')
  })

  it('writes the JSON representation and carries the token', async () => {
    const invoke = answering(() => null)
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await synra.storage.set('config', { a: 1 })

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'storage.set',
      args: { key: 'config', value: '{"a":1}' }
    })
  })

  it('refuses a value that JSON cannot represent', async () => {
    const invoke = answering(() => null)
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.storage.set('bad', undefined)).rejects.toMatchObject({ code: 'invalid_argument' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('passes the prefix to keys and omits it when there is none', async () => {
    const invoke = answering(() => ['a', 'ab'])
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.storage.keys('a')).resolves.toEqual(['a', 'ab'])
    await expect(synra.storage.keys()).resolves.toEqual(['a', 'ab'])

    expect(invoke).toHaveBeenNthCalledWith(1, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'storage.keys',
      args: { prefix: 'a' }
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'storage.keys',
      args: {}
    })
  })

  it('reports the quota the kernel measured', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(() => ({ usedBytes: 5, maxBytes: 64 })) }) }))

    await expect(synra.storage.quota()).resolves.toEqual({ usedBytes: 5, maxBytes: 64 })
  })

  it('keeps a kernel refusal a refusal', async () => {
    const invoke = answering(() => {
      throw Object.assign(new Error('no write capability'), {
        code: 'capability_denied',
        message: 'error.capabilityDenied'
      })
    })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.storage.set('k', 'v')).rejects.toMatchObject({ code: 'capability_denied' })
  })

  it('deletes through the channel', async () => {
    const invoke = answering(() => true)
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.storage.delete('k')).resolves.toBeUndefined()
    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'storage.delete',
      args: { key: 'k' }
    })
  })
})
