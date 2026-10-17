/**
 * Where kernel events become store changes.
 *
 * # Why this is a module rather than three lines in `main.ts`
 *
 * Because it is the one place that decides **which** event kinds this build understands, and that decision deserves a
 * name. `main.ts` is three lines on purpose; a dispatcher wired inline there would be the first exception, and the
 * second would be the store that registered itself.
 *
 * # What it wires
 *
 * The kernel emits on one channel (`KERNEL_EVENT_CHANNEL`). The dispatcher judges each envelope by its sequence
 * number — apply, ignore a repeat, or re-read a snapshot — and routes what it applies to a handler here. The device
 * store is the only store with an `apply(event)` today, so it is the only one registered.
 *
 * # Why an unhandled kind re-reads
 *
 * Because an interface that predates the kernel it is talking to cannot know what changed. Silently dropping the event
 * would leave a page that looks current and is not — which is the failure the sequence tracker exists to make visible.
 * The cost is one snapshot call per unknown event, which is what "I am out of date" costs.
 */

import type { DevicesSnapshot, SequencedKernelEvent } from '@synra/protocol'

import type { KernelClient } from './ipc/client'
import { KERNEL_EVENT_CHANNEL } from './ipc/channel'
import { EventDispatcher } from './ipc/dispatcher'
import { kernel } from './kernel'
import { useDevicesStore } from './stores/devices'
import { useIntentsStore } from './stores/intents'
import { usePluginsStore } from './stores/plugins'

/** What the caller keeps, so a test — or a future hot-reload — can stop the stream. */
export interface StartedEvents {
  /** The dispatcher, exposed for the counters a diagnostic screen will want. */
  readonly dispatcher: EventDispatcher
  /** Stops listening. The kernel keeps publishing; nobody is reading the channel. */
  readonly stop: () => Promise<void>
}

/**
 * Starts the event stream into the stores.
 *
 * `client` is a parameter rather than the imported singleton so a test supplies a fake bridge without touching
 * `window` — the same reason `bridgeFor` takes its candidate.
 */
export async function startEvents(client: KernelClient = kernel): Promise<StartedEvents> {
  const devices = useDevicesStore()

  /**
   * Re-reads the device snapshot and marks the picture whole again.
   *
   * A **declaration**, not an arrow const, and it takes the dispatcher explicitly: the refresh callback is created with
   * the dispatcher, so a closure over it would read a binding that does not exist yet — correct at call time, and the
   * kind of "works but reads as a bug" that a linter is right to flag. Passing it in says what the dependency is.
   *
   * A failed re-read leaves the picture stale on purpose: `snapshotTaken` marks it whole again, and marking a picture
   * whole when the call failed is the silent-staleness failure the sequence tracker exists to make visible.
   */
  async function reread(client: KernelClient, devices: ReturnType<typeof useDevicesStore>, dispatcher: EventDispatcher): Promise<void> {
    const answer = await client.call<DevicesSnapshot>('devices_snapshot', {})
    if (!answer.ok)
      return
    devices.applySnapshot(answer.value)
    dispatcher.snapshotTaken()
  }

  const dispatcher = new EventDispatcher(() => {
    void reread(client, devices, dispatcher)
  })

  dispatcher.on('deviceCardUpdated', (event) => {
    devices.apply(event)
  })
  dispatcher.on('deviceRemoved', (event) => {
    devices.apply(event)
  })
  // A scan is not a device: its progress is a transient the page reads from the scan state it already asked for, so the
  // response is to re-read rather than to model a second copy of it here.
  dispatcher.on('scanUpdated', () => {
    void reread(client, devices, dispatcher)
  })

  // ★ The two facts that had no consumer until now: an intent arriving and a transfer moving. `apply` answers whether the
  // store recognised the event, and a `false` here is a bug in this file rather than a missed event — every kind the
  // store knows is registered above, so the dispatcher's "unknown kind" path stays for kinds *this build* lacks.
  // ★ A plugin card arrives with the event, so the plugins page no longer re-reads the list to show it.
  const plugins = usePluginsStore()
  dispatcher.on('pluginUpdated', (event) => {
    plugins.apply(event)
  })

  const intents = useIntentsStore()
  dispatcher.on('intentUpdated', (event) => {
    intents.apply(event)
  })
  dispatcher.on('intentFinished', (event) => {
    intents.apply(event)
  })
  dispatcher.on('transferUpdated', (event) => {
    intents.apply(event)
  })
  dispatcher.on('transferFinished', (event) => {
    intents.apply(event)
  })

  const stop = await client.subscribe(KERNEL_EVENT_CHANNEL, (payload) => {
    dispatcher.accept(payload as SequencedKernelEvent)
  })

  return { dispatcher, stop: async () => stop() }
}
