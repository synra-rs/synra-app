/**
 * The device list, as a mirror of what the kernel says.
 *
 * # What a mirror store is, and what it is not
 *
 * The design document draws the line: these stores **cache a snapshot and apply incremental events**. They hold no
 * business logic — no "should this device be connected", no "should this plugin be installed". A rule like that lives
 * in the kernel, and a copy of it here would be a second implementation that disagrees with the first under load.
 *
 * So every function below either *records* what the kernel said or *answers a question about the recording*. Nothing
 * decides anything.
 *
 * # Why a list and an index
 *
 * The components render a list, and the transfer panel and the pairing dialog look a device up by id. A single array
 * with a `find` per lookup is O(n) on every render, and the list is small — but the *pairing* path looks up on every
 * incoming request, which arrives at the peer's rate rather than the user's. So the index is maintained alongside,
 * and the two are updated by the same two functions.
 */

import type {
  DeviceCard,
  DevicesSnapshot,
  IdentitySnapshot,
  KernelEvent,
  ScanState,
  TransferSummary
} from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export const useDevicesStore = defineStore('devices', () => {
  let identityRevision = 0
  let identityRead = 0
  const cards = ref<readonly DeviceCard[]>([])
  /**
   * The scan in progress, or nothing.
   *
   * The kernel's `scanning` is the scan *itself* rather than a boolean, which is why this is one field: a boolean
   * beside a progress value is two places the answer lives, and the day they disagree the interface shows "scanning"
   * with a stale percentage.
   */
  const scan = ref<ScanState | undefined>(undefined)
  /** This device, so a screen does not need a second call for its own name. */
  const identity = ref<IdentitySnapshot | undefined>(undefined)

  /**
   * The cards by id.
   *
   * Rebuilt by `applySnapshot` and maintained by `upsert` and `remove`, rather than derived on every read: a
   * `computed` over the list would rebuild the whole index whenever one card changed, and the pairing path reads it
   * per request.
   */
  const byId = computed(() => new Map(cards.value.map(card => [card.deviceId, card])))

  /** Records a whole snapshot, which is what the interface asks for after a gap. */
  function applySnapshot(snapshot: DevicesSnapshot): void {
    identityRevision++
    // Replaced rather than merged: a snapshot is the kernel's complete answer, so a card the snapshot does not carry
    // is one that is gone. Merging would keep a device the user has removed until the interface restarted.
    cards.value = snapshot.devices
    scan.value = snapshot.scanning
    identity.value = snapshot.identity
  }

  /** Keep page and shell on the same identity; late reads cannot replace a newer snapshot or read. */
  async function readIdentity(client: Pick<KernelClient, 'call'>): Promise<CallResult<IdentitySnapshot>> {
    const ticket = ++identityRead
    const revision = identityRevision
    const answer = await client.call<IdentitySnapshot>('identity_get')
    if (answer.ok && ticket === identityRead && revision === identityRevision) {
      identityRevision++
      identity.value = answer.value
    }
    return answer
  }

  /** Whether a scan is running, which is what the components ask. */
  const scanning = computed(() => scan.value !== undefined)

  /** Records one device, adding it when it is new. */
  function upsert(card: DeviceCard): void {
    const at = cards.value.findIndex(existing => existing.deviceId === card.deviceId)
    if (at >= 0) {
      const next = cards.value.slice()
      next[at] = card
      cards.value = next
      return
    }
    cards.value = [...cards.value, card]
  }

  /** Records that a device is gone. */
  function remove(deviceId: string): void {
    cards.value = cards.value.filter(card => card.deviceId !== deviceId)
  }

  /** Records a transfer's progress on the device it belongs to. */
  function setTransfer(deviceId: string, transfer: TransferSummary | undefined): void {
    const at = cards.value.findIndex(card => card.deviceId === deviceId)
    if (at < 0) {
      // A transfer for a device the list does not have is one the interface has not been told about yet. Dropping it
      // rather than inventing a card: a card with no name or platform would render as a blank row, and the snapshot
      // that arrives with the gap's answer will carry both.
      return
    }
    const next = cards.value.slice()
    const existing = next[at]
    if (!existing)
      return
    next[at] = transfer === undefined ? withoutTransfer(existing) : { ...existing, transfer }
    cards.value = next
  }

  /** The devices the user has paired with. */
  const paired = computed(() => cards.value.filter(card => card.paired))

  /** The paired devices that are reachable now. */
  const online = computed(() => cards.value.filter(card => card.paired && card.online))

  /** A device by id. */
  function get(deviceId: string): DeviceCard | undefined {
    return byId.value.get(deviceId)
  }

  /**
   * Applies one kernel event.
   *
   * Returns whether it was one this store handles, so the dispatcher's caller can tell a routed event from an
   * unhandled one without a second lookup. The dispatcher already decides that by handler registration; this is for the
   * one caller that wires them together.
   */
  function apply(event: KernelEvent): boolean {
    switch (event.type) {
      case 'deviceCardUpdated':
        upsert(event.payload)
        return true
      case 'deviceRemoved':
        remove(event.payload.deviceId)
        return true
      case 'scanUpdated':
        scan.value = event.payload
        return true
      default:
        return false
    }
  }

  return {
    cards,
    scanning,
    scan,
    identity,
    readIdentity,
    byId,
    paired,
    online,
    applySnapshot,
    upsert,
    remove,
    setTransfer,
    get,
    apply
  }
})

/** A card without its transfer, for the one place a transfer is cleared. */
function withoutTransfer(card: DeviceCard): DeviceCard {
  const { transfer: _transfer, ...rest } = card
  return rest
}
