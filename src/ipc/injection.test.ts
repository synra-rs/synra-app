import type { ClientFeature, SynraClient } from '@synra/client'

import type { ErrorCode } from '@synra/protocol'
import type { Bridge } from './client'

import type { InjectionContext } from './injection'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from './injection'

/**
 * A bridge that answers nothing and records what it was asked.
 *
 * Every test needs a different answer, so the ones that care about a specific command replace `invoke`; the ones that
 * care about refusals leave this one.
 */
function bridge(overrides: Partial<Bridge> = {}): Bridge {
  return {
    invoke: vi.fn(async () => ({})),
    listen: vi.fn(async () => () => {
      // A subscription that can be dropped.
    }),
    ...overrides
  }
}

function context(overrides: Partial<InjectionContext> = {}): InjectionContext {
  return {
    bridge: bridge(),
    pluginId: 'com.example.notes',
    granted: [],
    deniedCapabilities: [],
    locale: 'zh-CN',
    darkMode: false,
    sdkVersion: '0.1.0',
    ...overrides
  }
}

/** The call a feature name stands for, so the test can ask the object to do the thing. */
const CALLS: Readonly<Record<ClientFeature, (synra: SynraClient) => Promise<unknown>>> = {
  'event.subscribe': async (synra) => {
    synra.event.subscribe('anything', () => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
  },
  'device.onChange': async (synra) => {
    synra.device.onChange(() => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
  },
  'device.pick': async synra => synra.device.pick(),
  'transfer.onProgress': async (synra) => {
    // ★ The id comes first. A call written the other way round typechecks against a one-argument signature and then
    // subscribes nothing — the failure this file exists to prevent, in the test for the guard itself.
    synra.transfer.onProgress('transfer-1', () => {
      // Nothing is done with it.
    })
  },
  'network.fetch': async synra => synra.network.fetch('https://example.invalid'),
  'clipboard.read': async synra => synra.clipboard.read()
}

/** Every feature the interface declares, so a new one cannot be added without a test noticing. */
const FEATURES: readonly ClientFeature[] = [
  'device.onChange',
  'device.pick',
  'transfer.onProgress',
  'event.subscribe',
  'network.fetch',
  'clipboard.read'
]

describe('the injected object', () => {
  it('has every namespace and no more', () => {
    // ★ The reason this file is written the way it is.
    //
    // `satisfies` in the source is what enforces this at compile time; this test is what makes the enforcement
    // *visible* to someone who reads the test output and never opens the type checker.
    const synra = createInjection(context())
    expect(Object.keys(synra).sort()).toEqual([
      'clipboard',
      'device',
      'env',
      'event',
      'isCode',
      'isSynraError',
      'log',
      'network',
      'platform',
      'requestId',
      'self',
      'storage',
      'transfer'
    ])
  })
})

describe('what the host advertises and what it does', () => {
  // ★ The invariant this whole file is built around, tested as the biconditional it actually is.
  //
  // A host that reports `hostSupports('event.subscribe') === true` and then rejects the call is worse than one that
  // never claimed it: the author writes the branch, it passes review, and the failure lands on a user instead of in a
  // review. The other direction matters just as much — a call that quietly works while the table says it does not
  // leaves an author guarding against something that never fails, and refusing the API that would have worked.
  //
  // So: advertised means it works, and not-advertised means it refuses. Both halves, for every feature the interface
  // declares, so a feature added to the library without a decision here shows up as a missing entry.
  for (const feature of FEATURES) {
    it(`\`${feature}\` is advertised exactly when the call works`, async () => {
      // The grants that remove "denied" from the picture, so what is left is "the host cannot do this yet" and not
      // "this plugin was not allowed".
      const synra = createInjection(context({ granted: ['device.pick', 'clipboard_read'] }))
      const advertised = synra.env.hostSupports(feature)

      const failure = await CALLS[feature](synra).then(
        () => undefined,
        (reason: unknown) => reason as { code?: ErrorCode }
      )

      if (advertised)
        expect(failure, `${feature} is advertised but the call rejected`).toBeUndefined()
      else
        expect(failure, `${feature} is not advertised but the call worked`).toBeDefined()
    })
  }

  it('keeps the device list and the supported list in step', () => {
    const synra = createInjection(context())
    expect([...synra.device.supported].sort()).toEqual(
      FEATURES.filter(feature => synra.env.hostSupports(feature)).sort()
    )
  })
})

describe('what the host cannot do yet', () => {
  // ★ Refuse, do not fake.
  //
  // Each of these returns a rejected promise carrying `unsupported`. The alternative — an empty array, a made-up id, a
  // resolved promise — would let an author's code run to the point where it renders something that was never true.
  const cases: readonly { readonly name: string, readonly call: (synra: SynraClient) => Promise<unknown> }[] = [
    { name: 'device.send', call: async synra => synra.device.send({ to: 'peer', event: 'thing' }) },
    { name: 'device.broadcast', call: async synra => synra.device.broadcast({ event: 'thing' }) },
    { name: 'storage.get', call: async synra => synra.storage.get('key') },
    { name: 'storage.get with a fallback', call: async synra => synra.storage.get('key', { fallback: 0 }) },
    { name: 'storage.set', call: async synra => synra.storage.set('key', 1) },
    { name: 'storage.delete', call: async synra => synra.storage.delete('key') },
    { name: 'storage.keys', call: async synra => synra.storage.keys() },
    { name: 'storage.quota', call: async synra => synra.storage.quota() },
    {
      name: 'transfer.send',
      call: async synra =>
        synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt' })
    },
    { name: 'transfer.status', call: async synra => synra.transfer.status('t1') },
    { name: 'platform.openUrl', call: async synra => synra.platform.openUrl('https://example.invalid') },
    { name: 'platform.notify', call: async synra => synra.platform.notify({ title: 't', body: 'b' }) },
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

describe('capabilities the plugin was refused', () => {
  // ★ The refusal names the capability, and says "denied" rather than "unsupported".
  //
  // Those are different problems with different fixes: a denied capability is a question for the user, an unsupported
  // one is a question for the host. Merging them means the author cannot tell whether asking again would help.
  it('clipboard.read is refused as denied without the grant', async () => {
    const synra = createInjection(context({ granted: [] }))
    await expect(synra.clipboard.read()).rejects.toMatchObject({ code: 'capability_denied' })
  })

  it('clipboard.read is refused as unsupported with the grant, because no command exists', async () => {
    const synra = createInjection(context({ granted: ['clipboard_read'] }))
    await expect(synra.clipboard.read()).rejects.toMatchObject({ code: 'unsupported' })
  })

  it('device.pick is refused as denied without the grant', async () => {
    const synra = createInjection(context({ granted: [] }))
    await expect(synra.device.pick()).rejects.toMatchObject({ code: 'capability_denied' })
  })
})

describe('the three "can"s mean three different things', () => {
  // ★ The library's own comment warns that mixing these makes a button's disabled condition wrong. So they are tested
  // apart: a capability the platform cannot do but the user granted is still granted, and asking about the grant must
  // not be answered by the platform.
  it('can answers about the grant', () => {
    const synra = createInjection(context({ granted: ['clipboard_read'] }))
    expect(synra.env.can('clipboard_read')).toBe(true)
    expect(synra.env.can('clipboard_write')).toBe(false)
  })

  it('platformSupports answers about the platform, never the grant', () => {
    const granted = createInjection(context({ granted: ['clipboard_read'] }))
    const refused = createInjection(context({ granted: [] }))
    expect(granted.env.platformSupports('clipboard_read'))
      .toBe(refused.env.platformSupports('clipboard_read'))
  })

  it('hostSupports answers about the host version', () => {
    const synra = createInjection(context())
    expect(synra.env.hostSupports('event.subscribe')).toBe(true)
    expect(synra.env.hostSupports('network.fetch')).toBe(false)
  })
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
  it('answers with the online devices the kernel named', async () => {
    const answering = bridge({
      invoke: vi.fn(async () => ({
        devices: [
          { deviceId: 'a', displayName: 'Phone', platform: 'android', online: true },
          { deviceId: 'b', displayName: 'Laptop', platform: 'windows', online: false }
        ]
      }))
    })
    const synra = createInjection(context({ bridge: answering }))
    await expect(synra.device.list()).resolves.toEqual([
      { deviceId: 'a', displayName: 'Phone', platform: 'android', online: true }
    ])
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

describe('error narrowing', () => {
  it('recognises its own error and rejects an impostor', () => {
    const synra = createInjection(context())
    const real = { code: 'capability_denied', message: 'error.capabilityDenied', retryable: false }
    expect(synra.isSynraError(real)).toBe(true)
    expect(synra.isCode(real, 'capability_denied')).toBe(true)
    expect(synra.isCode(real, 'timeout' as ErrorCode)).toBe(false)
    expect(synra.isSynraError({ code: 1 })).toBe(false)
    expect(synra.isSynraError(undefined)).toBe(false)
    expect(synra.isSynraError('capability_denied')).toBe(false)
  })
})

describe('requestId', () => {
  // ★ From the host, and distinct per call, because the identifier goes into the kernel's de-duplication table.
  it('is unique per call and carries the plugin', () => {
    const synra = createInjection(context())
    const first = synra.requestId()
    const second = synra.requestId()
    expect(first).not.toBe(second)
    expect(first).toContain('com.example.notes')
  })
})

describe('logging', () => {
  it('prefixes a child logger and nests it', () => {
    const lines: string[] = []
    const spy = vi.spyOn(console, 'info').mockImplementation((line: unknown) => {
      lines.push(String(line))
      return undefined
    })
    try {
      const synra = createInjection(context())
      synra.log.child('net').info('connected')
      synra.log.child('net').child('retry').info('again')
      expect(lines).toEqual(['net connected', 'net.retry again'])
    }
    finally {
      spy.mockRestore()
    }
  })
})
