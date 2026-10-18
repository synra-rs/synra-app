/**
 * What the host cannot do yet, and the guarantees it makes while refusing.
 *
 * # Refuse, do not fake
 *
 * Each call in the first group returns a rejected promise carrying `unsupported`. The alternative — an empty array, a
 * made-up id, a resolved promise — would let an author's code run to the point where it renders something that was
 * never true.
 *
 * # And always a function
 *
 * `subscribe` and `onProgress` hand back an unsubscribe even when nothing was attached, because an author who has to
 * write `maybe?.()` is an author who will leak the subscription. The race — the caller unsubscribing before the
 * listener finishes attaching — is asserted here too, because it shows up much later as "my component updates after it
 * unmounted".
 *
 * The surface is asserted in `injection-surface.test.ts` and the advertised/actual biconditional in
 * `injection-capabilities.test.ts`; the factories all three share are in `injection-test-helpers.ts`.
 */

import type { SynraClient } from '@synra/client'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

describe('what the host cannot do yet', () => {
  const cases: readonly { readonly name: string, readonly call: (synra: SynraClient) => Promise<unknown> }[] = [
    { name: 'device.send', call: async synra => synra.device.send({ to: 'peer', event: 'thing' }) },
    { name: 'device.broadcast', call: async synra => synra.device.broadcast({ event: 'thing' }) },
    {
      name: 'transfer.send',
      call: async synra =>
        synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt' })
    },
    { name: 'transfer.status', call: async synra => synra.transfer.status('t1') },
    { name: 'self.invoke', call: async synra => synra.self.invoke('doThing') },
    { name: 'self.actions', call: async synra => synra.self.actions() },
    { name: 'event.emit', call: async synra => synra.event.emit('_plugin.com.example.notes.thing') },
    { name: 'network.fetch', call: async synra => synra.network.fetch('https://example.invalid') }
  ]

  for (const { name, call } of cases) {
    it(`${name} refuses with \`unsupported\``, async () => {
      const synra = createInjection(context({ granted: ['*'] }))
      await expect(call(synra)).rejects.toMatchObject({ code: 'unsupported' })
    })
  }
})

describe('subscriptions', () => {
  // ★ Always a function, so an author never has to write `maybe?.()` and never leaks one.
  it('subscribe returns an unsubscribe even when the bridge refuses to attach', async () => {
    const failing = bridge({
      listen: vi.fn(async () => {
        throw new Error('no listener')
      })
    })
    const synra = createInjection(context({ bridge: failing }))
    const off = synra.event.subscribe('anything', () => {
      // Nothing is done with it.
    })
    expect(typeof off).toBe('function')
    expect(() => off()).not.toThrow()
  })

  it('onProgress returns an unsubscribe even though transfers do not start', () => {
    const synra = createInjection(context())
    const off = synra.transfer.onProgress('transfer-1', () => {
      // Nothing is done with it.
    })
    expect(typeof off).toBe('function')
    expect(() => off()).not.toThrow()
  })

  // ★ The race: the caller unsubscribes before the listener finishes attaching. The one that arrives afterwards has to
  // be dropped, or the handler keeps firing for a subscription the author already cancelled — which shows up much
  // later as "my component updates after it unmounted".
  it('drops the listener that attaches after the caller unsubscribed', async () => {
    let release: (off: () => void) => void = () => {
      // Assigned below.
    }
    const slow = bridge({
      listen: vi.fn(() => new Promise<() => void>((resolve) => {
        release = resolve
      }))
    })
    const synra = createInjection(context({ bridge: slow }))
    const handler = vi.fn()
    const off = synra.event.subscribe('anything', handler)
    off()
    const attached = vi.fn()
    release(attached)
    await Promise.resolve()
    expect(attached).toHaveBeenCalled()
  })
})

describe('device.list', () => {
  it('asks the plugin channel with its token and answers with the online devices', async () => {
    // ★ The channel, not just the answer. `devices_snapshot` is a *host* command; a plugin page reaching it would be the
    // separation the two interfaces exist to prevent. The method, the token and the empty args are the whole call.
    const invoke = vi.fn(async () => [
      { deviceId: 'a', displayName: 'Phone', platform: 'android', online: true },
      { deviceId: 'b', displayName: 'Laptop', platform: 'windows', online: false }
    ])
    const ctx = context({ bridge: bridge({ invoke }) })
    const synra = createInjection(ctx)

    await expect(synra.device.list()).resolves.toEqual([
      { deviceId: 'a', displayName: 'Phone', platform: 'android', online: true }
    ])
    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: ctx.token,
      method: 'device.list',
      args: {}
    })
  })

  // ★ A refusal stays a refusal.
  //
  // Returning `[]` would be the worst of both: the author's `try` never fires, the picker says "no devices", and the
  // real reason is nowhere.
  it('propagates the kernel refusal instead of answering with an empty list', async () => {
    const refusing = bridge({
      invoke: vi.fn(async () => {
        // An `Error` carrying the fields, because that is what a command actually rejects with and what the injection's
        // error mapping reads. A bare object literal would be the same assertion written in a form nothing produces.
        throw Object.assign(new Error('the store is not reachable'), {
          code: 'store_unavailable',
          message: 'error.storeUnavailable',
          retryable: true
        })
      })
    })
    const synra = createInjection(context({ bridge: refusing }))
    await expect(synra.device.list()).rejects.toMatchObject({ code: 'store_unavailable' })
  })
})
