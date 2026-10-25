import type { CredentialCleanupReport, CredentialRepairReport, CredentialSnapshot, DevicesSnapshot, IpcError } from '@synra/protocol'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { kernel } from '../kernel'
import { useDevicesStore } from './devices'

/** Persisted enrollment, current-process readiness and operation results have separate lifetimes. */
export const useCredentialsStore = defineStore('credentials', () => {
  const snapshot = ref<CredentialSnapshot | null>(null)
  const repaired = ref<CredentialRepairReport | null>(null)
  const cleanup = ref<CredentialCleanupReport | null>(null)
  const busy = ref(false)
  const commandError = ref<IpcError>()
  const statusError = ref<IpcError>()
  const metadataError = ref<IpcError>()
  const error = computed(() => commandError.value ?? metadataError.value ?? statusError.value)
  const restartRequired = computed(() => snapshot.value?.restartRequired === true || repaired.value?.restartRequired === true)
  let revision = 0
  let reading: Promise<boolean> | undefined

  function refresh(): Promise<boolean> {
    revision += 1
    if (reading !== undefined)
      return reading
    reading = readLatest().finally(() => {
      reading = undefined
    })
    return reading
  }

  async function readLatest(): Promise<boolean> {
    while (true) {
      const started = revision
      const answer = await kernel.call<CredentialSnapshot>('identity_credentials')
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

  async function repair(expectedFingerprint: string): Promise<boolean> {
    if (busy.value)
      return false
    busy.value = true
    commandError.value = undefined
    revision += 1
    try {
      const answer = await kernel.call<CredentialRepairReport>('identity_repair', { expectedFingerprint })
      if (answer.ok) {
        repaired.value = answer.value
        cleanup.value = answer.value.cleanup
      }
      else {
        commandError.value = answer.error
      }
      // A failed external write can already have revoked grants; re-read both facts on either outcome.
      await refreshDevices()
      await refresh()
      return answer.ok
    }
    finally {
      busy.value = false
    }
  }

  async function refreshDevices(): Promise<void> {
    const devices = await kernel.call<DevicesSnapshot>('devices_snapshot')
    if (devices.ok) {
      useDevicesStore().applySnapshot(devices.value)
      metadataError.value = undefined
    }
    else {
      metadataError.value = devices.error
    }
  }

  async function reload(): Promise<void> {
    commandError.value = undefined
    await Promise.all([refresh(), refreshDevices()])
  }

  async function retryCleanup(): Promise<void> {
    if (busy.value)
      return
    busy.value = true
    commandError.value = undefined
    try {
      const answer = await kernel.call<CredentialCleanupReport>('identity_cleanup')
      if (answer.ok)
        cleanup.value = answer.value
      else
        commandError.value = answer.error
      await refresh()
    }
    finally {
      busy.value = false
    }
  }

  return { snapshot, repaired, cleanup, busy, error, restartRequired, refresh, reload, repair, retryCleanup }
})
