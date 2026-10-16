/**
 * The device namespace: the only group that answers from the kernel today.
 *
 * # Why a refusal stays a refusal here
 *
 * `list` is the one method with a real command behind it, and swallowing a kernel error into an empty list would be the
 * worst of both: the author's `try` never fires, the picker shows "no devices", and the real reason — a refused command,
 * a wedged store — is nowhere.
 */

import type {
  ClientFeature,
  DeviceNamespace,
  DeviceTarget,
  EventNamespace,
  PickOptions
} from '@synra/client'
import type { DeviceId } from '@synra/protocol'

import type { InjectionContext } from './context'
import { BACKED } from './capabilities'
import { asSynraError, denied, unsupported } from './errors'

/** The device namespace, over the bridge and the event namespace it reports changes through. */
export function createDeviceNamespace(
  context: InjectionContext,
  event: EventNamespace,
  granted: ReadonlySet<string>
): DeviceNamespace {
  return {
    // ★ The only namespace that answers from the kernel today.
    list: async (): Promise<DeviceTarget[]> => {
      const snapshot = await context.bridge
        .invoke('devices_snapshot')
        // ★ A kernel refusal stays a refusal.
        //
        // Swallowing it and returning an empty list would be the worst of both: the author's `try` never fires, the
        // picker shows "no devices", and the real reason — a refused command, a wedged store — is nowhere.
        .catch((reason: unknown) => {
          throw asSynraError(reason)
        }) as {
        readonly devices?: readonly {
          readonly deviceId: string
          readonly displayName: string
          readonly platform: string
          readonly online: boolean
        }[]
      }
      return (snapshot.devices ?? [])
        .filter(card => card.online)
        .map(card => ({
          deviceId: card.deviceId as DeviceId,
          displayName: card.displayName,
          platform: card.platform as DeviceTarget['platform'],
          online: card.online
        }))
    },
    send: async () => {
      throw unsupported('device.send')
    },
    broadcast: async () => {
      throw unsupported('device.broadcast')
    },
    onChange: (handler: (devices: DeviceTarget[]) => void) => {
      let latest: DeviceTarget[] = []
      return event.subscribe<{ readonly devices?: unknown }>('device.changed', (payload) => {
        latest = (payload.devices as DeviceTarget[] | undefined) ?? latest
        handler(latest)
      })
    },
    // ★ Exists and refuses, on purpose.
    //
    // The library's own comment: an author who calls a missing method gets `TypeError: not a function`, which names
    // neither side and cannot be handled. A refused method is an ordinary `catch`.
    pick: async (_options?: PickOptions): Promise<DeviceId | null> => {
      if (!granted.has('device.pick'))
        throw denied('device.pick')
      throw unsupported('device.pick')
    },
    // ★ Derived from the same table `hostSupports` reads.
    supported: (Object.keys(BACKED) as ClientFeature[]).filter(feature => BACKED[feature] === true)
  }
}
