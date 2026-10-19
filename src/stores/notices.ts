/**
 * The one line the kernel asks the interface to say, outside any page.
 *
 * The `notice` event has existed since the event contract was written and had no consumer: a received file finished, the
 * progress row disappeared, and nothing told the person who received it. The store keeps the **latest** notice rather than
 * a list — this is a banner, not a mailbox, and a queue of "Received …" would compete with the page the user is reading.
 */

import type { KernelEvent } from '@synra/protocol'
import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface Notice {
  /** The kernel's message key, e.g. `notice.received`. */
  readonly message: string
  /** What the sentence needs; shaped by the message key, not by this store. */
  readonly params: Readonly<Record<string, unknown>>
}

export const useNoticesStore = defineStore('notices', () => {
  const latest = ref<Notice | undefined>(undefined)

  /** Applies one event. Answers whether this store knew what it was. */
  function apply(event: KernelEvent): boolean {
    if (event.type !== 'notice')
      return false
    const params = event.payload.params
    latest.value = {
      message: event.payload.message,
      params: typeof params === 'object' && params !== null ? params as Record<string, unknown> : {}
    }
    return true
  }

  function dismiss(): void {
    latest.value = undefined
  }

  return { apply, dismiss, latest }
})
