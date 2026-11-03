import type { DevicesSnapshot, PairingSnapshot, ScanOptions, ScanState, SequencedKernelEvent } from '@synra/protocol'
import { DEVICES, IDENTITY } from './fake-kernel-fixtures'

export class PreviewDevices {
  #devices = DEVICES.map(device => ({ ...device }))
  #pairing: PairingSnapshot = { phase: 'idle' }
  #scanTimer: ReturnType<typeof setInterval> | undefined
  #scan: ScanState | undefined
  #scanNumber = 0
  #scanCancelled = false
  constructor(private emit: (type: SequencedKernelEvent['type'], payload: unknown) => void) {
    if (typeof location !== 'undefined' && new URLSearchParams(location.search).get('pairing') === 'asked') {
      const peer = this.#devices[1]!
      this.#pairing = { phase: 'asked', ask: { ...peer, code: '813642' } }
    }
  }

  invoke(command: string, args: Readonly<Record<string, unknown>> = {}): unknown {
    switch (command) {
      case 'identity_get': return { ...IDENTITY }
      case 'devices_snapshot': return { identity: { ...IDENTITY }, devices: this.#devices.map(device => ({ ...device })), ...(this.#scan ? { scanning: { ...this.#scan } } : {}) } satisfies DevicesSnapshot
      case 'devices_scan_start': {
        const opts = args.opts as ScanOptions
        return opts.cachedOnly || opts.timeoutMs === 0 ? 'scan-cached' : this.#startScan()
      }
      case 'devices_scan_cancel':
        if (!this.#scan || this.#scan.scanId !== args.scanId)
          throw Object.assign(new Error('error.invalid_argument'), { code: 'invalid_argument', retryable: false, params: { scanId: args.scanId, running: this.#scan?.scanId }, actions: [] })
        this.#scanCancelled = true
        return null
      case 'devices_pairing_snapshot': return this.#pairing
      case 'devices_pair': {
        const device = this.#devices.find(device => device.deviceId === args.deviceId)
        if (!device || device.paired || device.blocked)
          return { code: 'invalid_argument', message: 'error.badArgument', retryable: false }
        this.#pairing = { phase: 'waiting', offer: { ...device, code: '813642' } }
        this.emit('pairingUpdated', this.#pairing)
        return this.#pairing
      }
      case 'devices_pairing_cancel':
        this.#pairing = { ...this.#pairing, phase: 'done', outcome: 'cancelled' }
        this.emit('pairingUpdated', this.#pairing)
        return this.#pairing
      case 'devices_pairing_respond': {
        const id = this.#pairing.ask?.deviceId
        const device = this.#devices.find(device => device.deviceId === id)
        if (device) {
          device.paired = args.accepted === true
          device.blocked = args.block === true
          this.emit('deviceCardUpdated', { ...device })
        }
        this.#pairing = { ...this.#pairing, phase: 'done', outcome: args.accepted ? 'paired' : 'refused' }
        this.emit('pairingUpdated', this.#pairing)
        return this.#pairing
      }
      case 'devices_forget': {
        const device = this.#devices.find(device => device.deviceId === args.deviceId)
        if (device) {
          device.paired = false
          device.blocked = false
          this.emit('deviceCardUpdated', { ...device })
        }
        return null
      }
      default: return { code: 'unsupported', message: 'error.commandNotFound', retryable: false }
    }
  }

  #startScan(): string {
    if (this.#scan)
      throw Object.assign(new Error('error.rate_limited'), { code: 'rate_limited', retryable: false, params: { running: this.#scan.scanId }, actions: [] })
    const scanId = `preview-scan-${++this.#scanNumber}`
    const totalMs = 900
    const startedAt = Date.now()
    this.#scanCancelled = false
    this.#scan = { scanId, elapsedMs: 0, totalMs, found: 0 }
    this.emit('scanUpdated', this.#scan)

    this.#scanTimer = setInterval(() => {
      const elapsedMs = Date.now() - startedAt
      if (this.#scanCancelled || elapsedMs >= totalMs) {
        const cancelled = this.#scanCancelled
        this.#stopScan()
        this.emit('scanUpdated', { scanId, elapsedMs, totalMs, found: cancelled ? 0 : 1 })
        if (!cancelled)
          this.emit('deviceCardUpdated', { ...DEVICES[1]!, lastSeenMs: Date.now() })
        return
      }
      this.#scan = { scanId, elapsedMs, totalMs, found: 0 }
      this.emit('scanUpdated', this.#scan)
    }, 300)
    return scanId
  }

  #stopScan(): void {
    this.#scan = undefined
    if (this.#scanTimer !== undefined) {
      clearInterval(this.#scanTimer)
      this.#scanTimer = undefined
    }
  }
}
