import type { InputSnapshot, IpcError, KernelEvent } from '@synra/protocol'
import type { KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { kernel } from '../app/kernel'

/** One host-only receiver prompt, updated by the central sequenced event stream. */
export const useInputStore = defineStore('input', () => {
  const snapshot = ref<InputSnapshot>({})
  const busy = ref(false)
  const error = ref<IpcError>()
  let revision = 0

  function apply(event: KernelEvent): boolean {
    if (event.type !== 'inputUpdated')
      return false
    revision++
    snapshot.value = event.payload
    error.value = undefined
    return true
  }

  async function read(client: KernelClient = kernel): Promise<void> {
    const ticket = ++revision
    const answer = await client.call<InputSnapshot>('devices_input_snapshot')
    if (answer?.ok && ticket === revision)
      snapshot.value = answer.value
  }

  async function decide(allow: boolean): Promise<void> {
    const sessionId = snapshot.value.session?.sessionId
    if (!sessionId || busy.value || snapshot.value.session?.phase !== 'pending')
      return
    busy.value = true
    error.value = undefined
    try {
      const answer = await kernel.call<null>('devices_input_respond', { sessionId, allow })
      if (!answer.ok && snapshot.value.session?.sessionId === sessionId)
        error.value = answer.error
    }
    finally { busy.value = false }
  }

  async function stop(): Promise<void> {
    const sessionId = snapshot.value.session?.sessionId
    if (!sessionId || busy.value)
      return
    busy.value = true
    try {
      const answer = await kernel.call<null>('devices_input_stop', { sessionId })
      if (!answer.ok && snapshot.value.session?.sessionId === sessionId)
        error.value = answer.error
    }
    finally { busy.value = false }
  }

  return { snapshot, busy, error, apply, read, decide, stop }
})
