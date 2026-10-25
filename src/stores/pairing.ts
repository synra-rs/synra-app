import type { IpcError, PairingSnapshot } from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'

import { computed, ref } from 'vue'
import { kernel } from '../kernel'

/** Runtime progress is read from the shared service, independent of page lifetime and event delivery. */
export const usePairingStore = defineStore('pairing', () => {
  const snapshot = ref<PairingSnapshot | null>(null)
  const invitation = ref('')
  const busy = ref(false)
  const commandError = ref<IpcError>()
  const statusError = ref<IpcError>()
  const error = computed(() => commandError.value ?? statusError.value)
  let revision = 0
  let reading: Promise<boolean> | undefined

  const active = computed(() => snapshot.value !== null
    && ['listening', 'connecting', 'confirming', 'cancelling'].includes(snapshot.value.phase))

  /** Serialize reads and discard a result invalidated by an event or a completed command. */
  function refresh(client: KernelClient = kernel): Promise<boolean> {
    revision += 1
    if (reading !== undefined)
      return reading
    reading = readLatest(client).finally(() => {
      reading = undefined
    })
    return reading
  }

  async function readLatest(client: KernelClient): Promise<boolean> {
    while (true) {
      const started = revision
      const answer = await client.call<PairingSnapshot | null>('pairing_status')
      if (started !== revision)
        continue
      if (!answer.ok) {
        statusError.value = answer.error
        return false
      }
      statusError.value = undefined
      snapshot.value = answer.value
      return true
    }
  }

  async function run(operation: () => Promise<CallResult<unknown>>): Promise<boolean> {
    if (busy.value)
      return false
    busy.value = true
    commandError.value = undefined
    try {
      const answer = await operation()
      if (!answer.ok) {
        commandError.value = answer.error
        return false
      }
      return await refresh()
    }
    finally {
      busy.value = false
    }
  }

  async function exportInvitation(): Promise<void> {
    await run(async () => {
      const answer = await kernel.call<string>('pairing_invite')
      if (answer.ok)
        invitation.value = answer.value
      return answer
    })
  }

  async function listen(deviceId: string, imported: string): Promise<boolean> {
    return run(() => kernel.call<PairingSnapshot>('pairing_listen', { deviceId, invitation: imported.trim() }))
  }

  async function connect(deviceId: string, reply: string): Promise<boolean> {
    return run(() => kernel.call<PairingSnapshot>('pairing_connect', { deviceId, reply: reply.trim() }))
  }

  async function cancel(): Promise<boolean> {
    const current = snapshot.value
    if (current === null || !active.value)
      return false
    return run(() => kernel.call<null>('pairing_cancel', { requestId: current.requestId }))
  }

  return { snapshot, invitation, busy, error, active, refresh, exportInvitation, listen, connect, cancel }
})
