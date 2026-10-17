/**
 * The wiring: an envelope on the channel reaches a store, and a kind this build does not know makes it re-read.
 *
 * A dispatcher test already proves the sequence judgement. What is tested here is the **wiring** — that the channel is
 * subscribed, that a device event lands in the store, and that an unknown kind ends in a snapshot call rather than in
 * silence.
 */

import type { DeviceCard, SequencedKernelEvent } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { startEvents } from './events'
import { KERNEL_EVENT_CHANNEL } from './ipc/channel'
import { KernelClient } from './ipc/client'
import { DEVICES, PEER_ID } from './ipc/fake-kernel-fixtures'
import { useDevicesStore } from './stores/devices'
import { usePluginsStore } from './stores/plugins'

/**
 * A client over a bridge that records calls and hands back a listener.
 *
 * The snapshot call **fails** on purpose: `snapshotTaken` is only called when a snapshot arrived, so a failing re-read
 * is the case that must not mark the picture whole — and it keeps this file free of a `DevicesSnapshot` fixture that
 * only exists to be thrown away.
 */
function harness() {
  const listeners = new Map<string, (payload: unknown) => void>()
  const invoke = vi.fn(async () => {
    throw new Error('the kernel is not reachable in this test')
  })
  const client = new KernelClient({
    invoke,
    listen: async (event: string, handler: (payload: unknown) => void) => {
      listeners.set(event, handler)
      return () => listeners.delete(event)
    }
  } as never)
  return { client, invoke, listeners }
}

function card(id: string): DeviceCard {
  return { ...DEVICES[0], deviceId: id } as DeviceCard
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the event stream', () => {
  it('subscribes to the one channel the shell emits on', async () => {
    const { client, listeners } = harness()

    await startEvents(client)

    expect(listeners.has(KERNEL_EVENT_CHANNEL)).toBe(true)
  })

  it('applies a device card to the store', async () => {
    // ★ The end of the road this round added: shell emits → channel → dispatcher → store. Before it, the store only ever
    // heard from a page that asked for a snapshot.
    const { client, listeners } = harness()
    const store = useDevicesStore()

    await startEvents(client)
    listeners.get(KERNEL_EVENT_CHANNEL)?.({
      seq: 1,
      at: 1,
      type: 'deviceCardUpdated',
      payload: card(PEER_ID)
    } satisfies SequencedKernelEvent)

    expect(store.get(PEER_ID)?.deviceId).toBe(PEER_ID)
  })

  it('re-reads a snapshot when the kind is one this build does not handle', async () => {
    // Silence would leave the page looking current and being wrong — the failure the sequence tracker exists to make
    // visible. One extra call is the price of "I am out of date".
    const { client, invoke, listeners } = harness()

    await startEvents(client)
    invoke.mockClear()
    listeners.get(KERNEL_EVENT_CHANNEL)?.({
      seq: 1,
      at: 1,
      // ★ A kind **this build** does not register — not `pluginUpdated`, which was this test's example until the plugins
      // store learned it. The distinction the test is about is "no handler", which is a property of this file's wiring
      // rather than of the kernel's enum; a fabricated name is the honest way to say that, and the cast says it too.
      type: 'aKindThisBuildDoesNotKnow',
      payload: {}
    } as unknown as SequencedKernelEvent)

    expect(invoke).toHaveBeenCalledWith('devices_snapshot', {})
  })

  it('stops listening when the caller stops it', async () => {
    const { client, listeners } = harness()
    const started = await startEvents(client)

    await started.stop()

    expect(listeners.has(KERNEL_EVENT_CHANNEL)).toBe(false)
  })

  it('applies a plugin card to the plugins store', async () => {
    // ★ The wiring added when the plugins page stopped re-reading on every event: a `pluginUpdated` must reach the store
    // itself, or the page falls back to whatever it read at mount. Asserted on the store, not on a DOM.
    const { client, listeners } = harness()
    const plugins = usePluginsStore()

    await startEvents(client)
    listeners.get(KERNEL_EVENT_CHANNEL)?.({
      seq: 1,
      at: 1,
      type: 'pluginUpdated',
      payload: {
        pluginId: '@acme/hello',
        title: 'Hello',
        version: '1.0.0',
        publisher: 'acme',
        enabled: true,
        state: 'active',
        hybrid: false,
        granted: [],
        denied: [],
        requested: [],
      },
    } as unknown as SequencedKernelEvent)

    expect(plugins.plugin('@acme/hello')?.enabled).toBe(true)
  })
})
