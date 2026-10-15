/**
 * Choosing between the real bridge and the fake one, and what the real one adds on top of Tauri.
 *
 * # What the first half is for
 *
 * ★ The comment in `kernel.ts` said the Tauri bridge "is not written yet" and the file built the client over the fake
 * unconditionally. That is the shape of defect this repository keeps finding: a decision that was documented, is
 * observable in the source, and **nothing was ever in a position to fail** — because there was no second branch to get
 * wrong.
 *
 * The decision is testable only because it takes the candidate rather than reading the global: a test cannot install a
 * Tauri shell, but it can hand this function a page that has one.
 */

import type { TauriApi } from './tauri'

import { describe, expect, it, vi } from 'vitest'
import { FakeKernel } from './fake-kernel'
import { hasTauriInternals, TauriKernel } from './tauri'

/** A page the way Tauri leaves it: the low-level IPC injected, the ergonomic globals not. */
function aTauriPage(): object {
  return { __TAURI_INTERNALS__: { invoke: () => undefined } }
}

/** A page the way a browser leaves it. */
function aBrowserPage(): object {
  return { document: {}, window: {} }
}

/** An API that records what it was asked, so the bridge's own behaviour is observable. */
function aRecordingApi(): { api: TauriApi, calls: unknown[][], deliver: (payload: unknown) => void } {
  const calls: unknown[][] = []
  let deliver: (payload: unknown) => void = () => undefined
  return {
    calls,
    deliver: (payload) => {
      deliver(payload)
    },
    api: {
      invoke: (command, args) => {
        calls.push(args === undefined ? [command] : [command, args])
        return Promise.resolve({ ok: true })
      },
      listen: (event, handler) => {
        calls.push(['listen', event])
        deliver = (payload) => {
          handler({ payload })
        }
        return Promise.resolve(() => undefined)
      }
    }
  }
}

describe('telling a shell from a browser', () => {
  it('finds the internals when a shell injected them', () => {
    expect(hasTauriInternals(aTauriPage())).toBe(true)
  })

  it('finds nothing in a browser', () => {
    // The dev server, a component test, a storybook: all of them are this, and the interface has to keep working there.
    expect(hasTauriInternals(aBrowserPage())).toBe(false)
  })

  it('is not fooled by the property existing without being an object', () => {
    // ★ A page can define this itself — it is a plain property, so anything running on the page can set it. A check
    // written as `'__TAURI_INTERNALS__' in page` would answer "yes" to `{ __TAURI_INTERNALS__: true }` and hand the
    // interface a `TauriKernel` whose first call fails in a way that reads as a kernel fault.
    for (const value of [true, 1, 'yes', null]) {
      expect(hasTauriInternals({ __TAURI_INTERNALS__: value })).toBe(false)
    }
  })

  it('answers no for a candidate that is not an object at all', () => {
    // `globalThis` is always an object, but the parameter accepts anything and the check must not throw on the way to
    // saying no — a startup path that throws leaves the user with a blank window and no message.
    for (const value of [undefined, null, 0, 'window']) {
      expect(hasTauriInternals(value)).toBe(false)
    }
  })
})

describe('the bridge the interface actually gets', () => {
  it('is the Tauri one in a shell and the fake one in a browser', async () => {
    const { bridgeFor } = await import('../kernel')
    expect(bridgeFor(aTauriPage())).toBeInstanceOf(TauriKernel)
    expect(bridgeFor(aBrowserPage())).toBeInstanceOf(FakeKernel)
  })
})

describe('what the Tauri bridge adds on top of the API', () => {
  it('passes a bare command through without an argument object', async () => {
    // ★ A command with no parameters and a command handed `{}` are different calls on Tauri's side, so the bridge must
    // not helpfully supply an empty object.
    const { api, calls } = aRecordingApi()
    await new TauriKernel(() => Promise.resolve(api)).invoke('identity_get')

    expect(calls).toEqual([['identity_get']])
  })

  it('passes the arguments it was given', async () => {
    const { api, calls } = aRecordingApi()
    await new TauriKernel(() => Promise.resolve(api)).invoke('devices_forget', { deviceId: 'dev_1' })

    expect(calls).toEqual([['devices_forget', { deviceId: 'dev_1' }]])
  })

  it('unwraps the event Tauri delivers', async () => {
    // ★ The two bridges have to be interchangeable, and this is where they could stop being so. Tauri hands its handler
    // an envelope with the value under `.payload`, while `FakeKernel` delivers the payload itself. A `TauriKernel` that
    // forwarded the envelope would work in a shell and break in every test — the direction of failure that survives
    // longest, because the tests are the thing that would have caught it.
    const { api, deliver, calls } = aRecordingApi()
    const seen: unknown[] = []
    await new TauriKernel(() => Promise.resolve(api)).listen('kernel-event', (payload) => {
      seen.push(payload)
    })

    expect(calls).toEqual([['listen', 'kernel-event']])

    const envelope = { seq: 7, at: 0, type: 'logLine' }
    deliver(envelope)

    expect(seen).toEqual([envelope])
    expect(seen[0]).not.toHaveProperty('payload')
  })

  it('loads the API once rather than once per call', async () => {
    // The loader is a dynamic import, and a bridge that re-imported per call would make every command pay for a module
    // resolution. Asserted with a counter rather than by timing, because the cost is not what matters — the shape is.
    const load = vi.fn(() => Promise.resolve(aRecordingApi().api))
    const bridge = new TauriKernel(load)

    await bridge.invoke('identity_get')
    await bridge.invoke('plugins_list')
    await bridge.listen('kernel-event', () => undefined)

    expect(load).toHaveBeenCalledTimes(1)
  })

  it('shares one load between two calls that arrive together', async () => {
    // ★ Memoising the **promise** rather than the resolved value is what makes this true, and a test of sequential calls
    // cannot see the difference. Two commands issued in the same tick are the ordinary case — a page that loads and asks
    // for its identity and its devices at once — and without it each would start its own import.
    let resolve: (api: TauriApi) => void = () => undefined
    const load = vi.fn(
      () =>
        new Promise<TauriApi>((settle) => {
          resolve = settle
        })
    )
    const bridge = new TauriKernel(load)

    const first = bridge.invoke('identity_get')
    const second = bridge.invoke('devices_snapshot')
    resolve(aRecordingApi().api)
    await Promise.all([first, second])

    expect(load).toHaveBeenCalledTimes(1)
  })
})
