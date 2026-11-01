import type { IpcError, PairingSnapshot } from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'

import { computed, ref } from 'vue'
import { kernel } from '../kernel'

/** One device asking to pair, as the kernel reports it. */
export interface PendingRequest {
  readonly requestId: string
  readonly deviceId: string
  readonly displayName: string
  readonly platform: string
  readonly address: string
  readonly verificationCode: string
  readonly expiresInMs: number
}

/** Runtime progress is read from the shared service, independent of page lifetime and event delivery. */
export const usePairingStore = defineStore('pairing', () => {
  const snapshot = ref<PairingSnapshot | null>(null)
  const busy = ref(false)
  const refreshing = ref(false)
  const commandError = ref<IpcError>()
  const statusError = ref<IpcError>()
  /** What other devices are waiting on this person to decide about. */
  const pending = ref<PendingRequest[]>([])
  const pendingError = ref<IpcError>()
  /** The request currently being answered, so one choice does not disable another prompt's buttons. */
  const answering = ref<string | null>(null)
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
    refreshing.value = true
    reading = readLatest(client).finally(() => {
      reading = undefined
      refreshing.value = false
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

  /**
   * The requests other devices are waiting on this person to decide about.
   *
   * ★ Read from the kernel's shared desk rather than from an event. The desk is the thing both sides agree on, and it is
   * the only place a request exists: an event would be a second copy that could arrive late, or not at all, and a prompt
   * that never appeared is exactly the failure this list exists to end.
   */
  async function refreshPending(client: KernelClient = kernel): Promise<boolean> {
    const answer = await client.call<PendingRequest[]>('pairing_pending')
    if (!answer.ok) {
      pendingError.value = answer.error
      return false
    }
    pendingError.value = undefined
    pending.value = answer.value
    return true
  }

  /**
   * Accepts or declines one of them.
   *
   * `answering` holds the request being decided rather than a plain boolean: two prompts from two devices can be on
   * screen at once, and a single flag would put both buttons into a loading state for one person's choice.
   */
  async function answer(requestId: string, accept: boolean): Promise<boolean> {
    if (answering.value !== null)
      return false
    answering.value = requestId
    commandError.value = undefined
    try {
      const result = await kernel.call<null>('pairing_answer', { requestId, accept })
      if (!result.ok) {
        commandError.value = result.error
        return false
      }
      // The prompt is gone either way — accepted and recorded, or declined. Dismissed locally first so it cannot be
      // pressed twice while the list is being read again.
      pending.value = pending.value.filter(prompt => prompt.requestId !== requestId)
      return true
    }
    finally {
      answering.value = null
      await refreshPending()
      // An acceptance changes what is paired, so the device list is out of date the moment it succeeds.
      if (accept)
        await refresh()
    }
  }

  async function cancel(): Promise<boolean> {
    const current = snapshot.value
    if (current === null || !active.value)
      return false
    return run(() => kernel.call<null>('pairing_cancel', { requestId: current.requestId }))
  }

  return { snapshot, busy, refreshing, commandError, statusError, error, active, pending, pendingError, answering, refresh, refreshPending, answer, cancel }
})
