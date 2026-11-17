/**
 * The one line the kernel asks the interface to say, outside any page.
 *
 * The `notice` event has existed since the event contract was written and had no consumer: a received file finished, the
 * progress row disappeared, and nothing told the person who received it. The store keeps a small bounded list so a burst
 * of background events remains visible without turning the page into a mailbox.
 */

import type { KernelEvent } from '@synra/protocol'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export interface Notice {
  readonly id: number
  /** The kernel's message key, e.g. `notice.received`. */
  readonly message: string
  /** What the sentence needs; shaped by the message key, not by this store. */
  readonly params: Readonly<Record<string, unknown>>
}

export const useNoticesStore = defineStore('notices', () => {
  const items = ref<Notice[]>([])
  const dropped = ref(0)
  const latest = computed(() => items.value[0])
  let nextId = 0

  /** Applies one event. Answers whether this store knew what it was. */
  function apply(event: KernelEvent): boolean {
    if (event.type !== 'notice')
      return false
    const params = event.payload.params
    const notice: Notice = {
      id: ++nextId,
      message: event.payload.message,
      params: typeof params === 'object' && params !== null ? params as Record<string, unknown> : {}
    }
    if (items.value.length >= 3)
      dropped.value += 1
    items.value = [notice, ...items.value].slice(0, 3)
    return true
  }

  function dismiss(id?: number): void {
    if (id === undefined) {
      items.value = []
      dropped.value = 0
      return
    }
    items.value = items.value.filter(notice => notice.id !== id)
    if (items.value.length === 0)
      dropped.value = 0
  }

  return { apply, dismiss, items, dropped, latest }
})
