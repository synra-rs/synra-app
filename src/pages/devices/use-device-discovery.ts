import type { DevicesSnapshot, IpcError, ScanOptions } from '@synra/protocol'
import { computed, onScopeDispose, ref, watch } from 'vue'
import { kernel } from '../../app/kernel'
import { messages } from '../../messages'
import { useDevicesStore } from '../../stores/devices'

export function useDeviceDiscovery() {
  const devices = useDevicesStore()
  let disposed = false
  let readRevision = 0
  let reading: Promise<boolean> | undefined
  const snapshotError = ref<IpcError>()
  const refreshing = ref(false)
  const ready = ref(devices.identity !== undefined)
  function refresh(): Promise<boolean> {
    readRevision += 1
    if (reading)
      return reading
    refreshing.value = true
    reading = readLatest().finally(() => {
      reading = undefined
      refreshing.value = false
    })
    return reading
  }
  async function readLatest(): Promise<boolean> {
    while (true) {
      if (disposed)
        return false
      const revision = readRevision
      const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
      if (disposed)
        return false
      if (revision !== readRevision)
        continue
      snapshotError.value = answer.ok ? undefined : answer.error
      if (!answer.ok)
        return false
      devices.applySnapshot(answer.value)
      ready.value = true
      return true
    }
  }

  const NETWORK_SCAN: ScanOptions = { includePaired: false, cachedOnly: false }
  const CACHED_SCAN: ScanOptions = { includePaired: false, cachedOnly: true }
  const startingScan = ref(false)
  const cancellingScan = ref(false)
  const stopAccepted = ref(false)
  const scanState = computed(() => devices.scan)
  const scanning = computed(() => startingScan.value || devices.scanning || cancellingScan.value)
  const scanReport = ref<'finished' | 'stopRequested' | 'cached'>()
  const scanError = ref<IpcError>()
  const failedCancelId = ref<string>()
  let scanGeneration = 0
  const scanSummary = computed(() => scanning.value
    ? stopAccepted.value ? messages.devices.scanStopping : messages.devices.scanning
    : scanReport.value === 'cached'
      ? messages.devices.scanCached
      : scanReport.value === 'stopRequested' ? messages.devices.scanStopped : messages.devices.scanFinished)

  async function scan(): Promise<void> {
    if (scanning.value)
      return
    startingScan.value = true
    stopAccepted.value = false
    scanReport.value = undefined
    scanError.value = undefined
    failedCancelId.value = undefined
    try {
      const answer = await kernel.call<string>('devices_scan_start', { opts: NETWORK_SCAN })
      if (disposed)
        return
      if (!answer.ok) {
        if (answer.error.code !== 'unsupported') {
          scanError.value = answer.error
          return
        }
        const cached = await kernel.call<string>('devices_scan_start', { opts: CACHED_SCAN })
        if (disposed)
          return
        if (!cached.ok) {
          scanError.value = cached.error
          return
        }
        scanReport.value = 'cached'
      }
      const refreshed = await refresh()
      if (refreshed && !disposed && !devices.scanning && scanReport.value !== 'cached')
        scanReport.value = stopAccepted.value ? 'stopRequested' : 'finished'
    }
    finally {
      startingScan.value = false
    }
  }

  async function cancelScan(): Promise<void> {
    const id = scanState.value?.scanId
    if (!id || cancellingScan.value)
      return
    const generation = scanGeneration
    cancellingScan.value = true
    scanError.value = undefined
    failedCancelId.value = id
    try {
      const answer = await kernel.call<null>('devices_scan_cancel', { scanId: id })
      if (disposed)
        return
      if (!answer.ok) {
        scanError.value = answer.error
        return
      }
      stopAccepted.value = generation === scanGeneration && (!scanState.value || scanState.value.scanId === id)
      const refreshed = await refresh()
      if (refreshed && !disposed && !startingScan.value && !devices.scanning && stopAccepted.value)
        scanReport.value = 'stopRequested'
    }
    finally {
      cancellingScan.value = false
    }
  }

  async function retryScan(): Promise<void> {
    if (failedCancelId.value) {
      if (scanState.value?.scanId === failedCancelId.value)
        await cancelScan()
      else if (await refresh())
        scanError.value = undefined
    }
    else {
      await scan()
    }
  }
  watch(() => devices.scan?.scanId, (scanId, previousId) => {
    if (scanId && scanId !== previousId) {
      scanGeneration += 1
      stopAccepted.value = false
      scanReport.value = undefined
    }
  }, { flush: 'sync' })
  watch(() => devices.scanning, (running, wasRunning) => {
    if (wasRunning && !running && !startingScan.value && !cancellingScan.value && !scanError.value && scanReport.value !== 'cached')
      scanReport.value = stopAccepted.value ? 'stopRequested' : 'finished'
  })
  onScopeDispose(() => {
    disposed = true
    readRevision += 1
  })
  return { snapshotError, refreshing, ready, refresh, startingScan, cancellingScan, scanState, scanning, scanReport, scanError, failedCancelId, scanSummary, scan, cancelScan, retryScan }
}
