/**
 * The intents that have arrived and the transfers in flight.
 *
 * # Why one store for two kinds of fact
 *
 * Because the home page draws them together: "what this device was asked to do" and "what is moving" are two halves of
 * one answer, and two stores would mean the page watches two sources to decide when to redraw.
 *
 * # Why an event is applied rather than a snapshot taken
 *
 * The kernel already publishes one event per change (`intentUpdated`, `intentFinished`, `transferUpdated`,
 * `transferFinished`), so re-reading the list on every event would ask for what was just delivered. `apply` returns
 * whether it recognised the event, and an unrecognised kind is the caller's cue to re-read — the same contract the device
 * store has, so a caller can treat the two alike.
 */

import type { IntentSnapshot, KernelEvent, TransferSnapshot } from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useIntentsStore = defineStore('intents', () => {
  let transferRevision = 0
  let latestTransferRead = 0
  /** Intents that have arrived and are not finished, in the order the kernel first reported them. */
  const intents = ref<readonly IntentSnapshot[]>([])

  /** Transfers in flight, by identifier. */
  const transfers = ref<readonly TransferSnapshot[]>([])

  /** Replaces the whole picture, for the first read after startup. */
  function applySnapshot(next: { intents: readonly IntentSnapshot[], transfers: readonly TransferSnapshot[] }): void {
    transferRevision++
    intents.value = [...next.intents]
    transfers.value = [...next.transfers]
  }

  /** A manual recovery read cannot restore a request that a newer event already finished. */
  async function readTransfers(client: Pick<KernelClient, 'call'>): Promise<CallResult<readonly TransferSnapshot[]>> {
    const ticket = ++latestTransferRead
    const started = transferRevision
    const answer = await client.call<readonly TransferSnapshot[]>('transfer_list')
    if (answer.ok && ticket === latestTransferRead && started === transferRevision) {
      transferRevision++
      transfers.value = [...answer.value]
    }
    return answer
  }

  /** Applies one event. Answers whether this store knew what it was. */
  function apply(event: KernelEvent): boolean {
    switch (event.type) {
      case 'intentUpdated':
        intents.value = upsert(intents.value, event.payload, intent => intent.intentId)
        return true
      case 'intentFinished':
        intents.value = intents.value.filter(intent => intent.intentId !== event.payload.intentId)
        return true
      case 'transferUpdated':
        transferRevision++
        transfers.value = upsert(transfers.value, event.payload, transfer => transfer.transferId)
        return true
      case 'transferFinished':
        transferRevision++
        transfers.value = transfers.value.filter(transfer => transfer.transferId !== event.payload.transferId)
        return true
      default:
        return false
    }
  }

  /** One intent by identifier, for a page that has a link to it. */
  function intent(intentId: string): IntentSnapshot | undefined {
    return intents.value.find(intent => intent.intentId === intentId)
  }

  return { apply, applySnapshot, readTransfers, intent, intents, transfers }
})

/**
 * Replaces the entry with the same key, or appends it.
 *
 * Replacement in place rather than "remove then push": the order is what the panel draws, and a list that reshuffled
 * itself every time a candidate arrived would make the user lose the row they were reading.
 */
function upsert<T>(items: readonly T[], next: T, key: (item: T) => string): readonly T[] {
  const at = items.findIndex(item => key(item) === key(next))
  if (at < 0)
    return [...items, next]
  return items.map((item, index) => (index === at ? next : item))
}
