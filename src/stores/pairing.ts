import type { PairingPrompt } from '@synra/protocol'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

/**
 * The pairing requests waiting for this device's answer.
 *
 * # Why this is a store rather than page state
 *
 * Because a request arrives on its own schedule — a peer asks while the user is on another screen — and an answer has to
 * survive navigation. Page state would lose the prompt the moment the user looked at the plugins page, and the peer's
 * connection would go on waiting for a decision nobody can make any more.
 *
 * # What it deliberately does not hold
 *
 * Whether the user should accept. The code comparison is the security decision, and it is made by a person looking at two
 * screens — nothing here ranks devices, remembers past requests, or answers on the user's behalf.
 */
export const usePairingStore = defineStore('pairing', () => {
  const pending = ref<readonly PairingPrompt[]>([])

  /**
   * Records a request. A second event for the same request is ignored: the desk publishes once, and a duplicate row would ask
   * the user to compare the same code twice.
   */
  function announce(prompt: PairingPrompt): void {
    if (pending.value.some(entry => entry.requestId === prompt.requestId))
      return
    pending.value = [...pending.value, prompt]
  }

  /** Forgets a request, whichever side answered it. */
  function settle(requestId: string): void {
    pending.value = pending.value.filter(entry => entry.requestId !== requestId)
  }

  refresh()

  function refresh(): void {
    // Nothing to fetch: the desk pushes `pairingRequested` and this list is the record of what it pushed. Kept as a
    // named function so a caller that wants to re-read has one place to call, the way the other stores do.
  }

  const count = computed(() => pending.value.length)

  return { pending, count, announce, settle, refresh }
})
