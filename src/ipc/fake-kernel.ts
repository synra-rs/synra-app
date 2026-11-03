import type { SequencedKernelEvent } from '@synra/protocol'
import type { Bridge } from './client'
import { KERNEL_EVENT_CHANNEL } from './channel'
import {
  INTENT,
  INTENTS,
  TRANSFERS
} from './fake-kernel-fixtures'
import { PreviewConfig } from './preview-config'
import { PreviewDevices } from './preview-devices'
import { PreviewPlugins } from './preview-plugins'

export const REQUIRED_ARGS: Readonly<Record<string, readonly string[]>> = {
  config_set: ['patch'],
  devices_pair: ['deviceId'],
  devices_forget: ['deviceId'],
  devices_pairing_respond: ['accepted'],
  plugins_enable: ['pluginId'],
  plugins_disable: ['pluginId'],
  plugins_consent_respond: ['pluginId', 'granted'],
  plugins_install: ['source'],
  devices_scan_start: ['opts'],
  devices_scan_cancel: ['scanId'],
  intent_dispatch: ['source'],
  intent_decide: ['intentId', 'decision'],
  intent_snapshot: ['intentId'],
  transfer_respond: ['transferId', 'accept'],
  plugins_publisher_decide: ['publisherId', 'fingerprint', 'trust'],
  plugins_preview: ['pluginId'],
  plugins_check_update: ['pluginId'],
  plugins_update: ['pluginId'],
  plugins_reload: ['pluginId'],
  transfer_send_text: ['request'],
  transfer_send_file: ['request'],
  transfer_open: ['path'],
  transfer_reveal: ['path'],
  transfer_discard: ['path'],
  plugins_audit: ['pluginId', 'limit'],
  plugins_audit_clear: ['pluginId'],
  plugins_sync_plan: ['pluginId'],
  plugins_sync_run: ['plan'],
  plugins_catalog_search: ['query'],
  plugins_request_registry_search: ['query'],
  plugins_uninstall: ['pluginId', 'removeData'],
  diag_export: ['options']
}
export class FakeKernel implements Bridge {
  readonly #devices = new PreviewDevices((type, payload) => this.#emit(type, payload))
  readonly #plugins = new PreviewPlugins((type, payload) => this.#emit(type, payload))
  readonly #config = new PreviewConfig()
  readonly #handlers = new Map<string, Set<(payload: unknown) => void>>()
  #seq = 0
  #requireArguments(command: string, args?: Readonly<Record<string, unknown>>): void {
    for (const key of REQUIRED_ARGS[command] ?? []) {
      if (args?.[key] === undefined) {
        throw new Error(
          `invalid args \`${key}\` for command \`${command}\`: command ${command} missing required key ${key}`
        )
      }
    }
  }

  async invoke(command: string, args?: Readonly<Record<string, unknown>>): Promise<unknown> {
    this.#requireArguments(command, args)
    if (command.startsWith('devices_') || command === 'identity_get')
      return this.#devices.invoke(command, args)
    if (command.startsWith('plugins_'))
      return this.#plugins.invoke(command, args)
    switch (command) {
      case 'diag_export':
        return {
          path: 'synra-diagnostics.json',
          included: ['environment', 'config', 'devices', 'plugins'],
          excluded: []
        }
      case 'diag_logs_tail':
        return []
      case 'diag_crash_dismiss':
        return null
      case 'diag_crashes':
        return []
      case 'diag_probes':
        return {
          results: [
            { name: 'tcp_port_available', status: 'ok', detail: 'a free port was found' },
            {
              name: 'wasm_backend',
              status: 'warn',
              detail: 'this host did not read the runtime',
              suggestion: 'include this report when asking for help'
            }
          ]
        }
      case 'intent_dispatch':
        return '11111111-1111-4111-8111-111111111111'
      case 'intent_decide':
        return null
      case 'intent_list':
        return INTENTS.map(intent => ({ ...intent }))
      case 'intent_snapshot':
        return { ...INTENT }
      case 'transfer_respond':
        return {}
      case 'transfer_list':
        return TRANSFERS.map(transfer => ({ ...transfer }))
      case 'transfer_received':
        return []
      case 'transfer_open':
        return null
      case 'transfer_reveal':
        return null
      case 'transfer_discard':
        return null
      case 'transfer_send_text':
        return '55555555-5555-4555-8555-555555555555'
      case 'transfer_send_file':
        return '66666666-6666-4666-8666-666666666666'
      case 'config_get':
        return this.#config.snapshot()
      case 'config_set': {
        const result = this.#config.apply(args?.patch)
        if (typeof result === 'string')
          return { code: result === 'invalid' ? 'invalid_argument' : 'internal', message: result === 'invalid' ? 'error.badArgument' : 'error.storeUnavailable', retryable: false }
        this.#emit('configUpdated', result)
        return null
      }
      default:
        return {
          code: 'unsupported',
          message: 'error.commandNotFound',
          params: { command },
          retryable: false
        }
    }
  }

  async listen(event: string, handler: (payload: unknown) => void): Promise<() => void> {
    let handlers = this.#handlers.get(event)
    if (handlers === undefined) {
      handlers = new Set()
      this.#handlers.set(event, handlers)
    }
    handlers.add(handler)
    return () => {
      handlers?.delete(handler)
    }
  }

  #emit(type: SequencedKernelEvent['type'], payload: unknown): void {
    this.#seq += 1
    const event = { type, payload, seq: this.#seq, at: Date.now() }
    for (const handler of this.#handlers.get(KERNEL_EVENT_CHANNEL) ?? []) {
      handler(event)
    }
  }
}
