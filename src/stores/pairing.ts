import type { DeviceCard, IpcError, KernelEvent, PairingSnapshot } from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { kernel } from '../app/kernel'

/** One pairing prompt for the whole application, fed by the central event subscriber. */
export const usePairingStore = defineStore('pairing', () => {
  const snapshot = ref<PairingSnapshot>({ phase: 'idle' })
  const visible = ref(false)
  const peerName = ref('')
  const error = ref<IpcError>()
  const busy = ref(false)
  const unreachable = ref(false)
  let revision = 0

  function update(next: PairingSnapshot): void {
    revision += 1
    if (snapshot.value.phase === 'waiting' && next.phase === 'idle')
      unreachable.value = true
    peerName.value = next.offer?.displayName ?? next.ask?.displayName ?? peerName.value
    snapshot.value = next
    if (next.phase === 'asked' || next.phase === 'waiting') {
      visible.value = true
      unreachable.value = false
    }
  }

  function apply(event: KernelEvent): boolean {
    if (event.type !== 'pairingUpdated')
      return false
    update(event.payload)
    return true
  }

  async function read(client: KernelClient = kernel): Promise<void> {
    const before = revision
    const answer = await client.call<PairingSnapshot>('devices_pairing_snapshot')
    if (answer.ok && before === revision)
      update(answer.value)
  }

  async function perform(operation: () => Promise<CallResult<PairingSnapshot>>): Promise<boolean> {
    if (busy.value)
      return false
    busy.value = true
    error.value = undefined
    const before = revision
    try {
      const answer = await operation()
      if (!answer.ok) {
        error.value = answer.error
        return false
      }
      // A terminal event can arrive before the command's earlier waiting snapshot returns.
      if (before === revision)
        update(answer.value)
      return true
    }
    finally {
      busy.value = false
    }
  }

  async function start(card: DeviceCard): Promise<void> {
    if (busy.value || snapshot.value.phase === 'asked' || snapshot.value.phase === 'waiting')
      return
    peerName.value = card.displayName
    snapshot.value = { phase: 'idle' }
    unreachable.value = false
    visible.value = true
    await perform(() => kernel.call<PairingSnapshot>('devices_pair', { deviceId: card.deviceId }))
  }

  async function respond(accepted: boolean, block = false): Promise<void> {
    await perform(() => kernel.call<PairingSnapshot>('devices_pairing_respond', { accepted, block }))
  }

  async function cancel(): Promise<void> {
    await perform(() => kernel.call<PairingSnapshot>('devices_pairing_cancel'))
  }

  async function close(): Promise<void> {
    if (busy.value)
      return
    if (snapshot.value.phase === 'waiting' && !await perform(() => kernel.call<PairingSnapshot>('devices_pairing_cancel')))
      return
    if (snapshot.value.phase === 'asked' && !await perform(() => kernel.call<PairingSnapshot>('devices_pairing_respond', { accepted: false, block: false })))
      return
    visible.value = false
    error.value = undefined
  }

  return { snapshot, visible, peerName, error, busy, unreachable, apply, read, start, respond, cancel, close }
})
