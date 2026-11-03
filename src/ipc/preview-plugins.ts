import type { AuditEntryDto, CatalogItem, InstallPreviewDto, InstallReport, PluginSnapshot, SequencedKernelEvent, SyncPlanDto, SyncReport } from '@synra/protocol'
import { DEVICES, PLUGIN_AUDIT, PLUGINS } from './fake-kernel-fixtures'

/** Isolated browser fixtures; Native always calls the real bridge. */
export class PreviewPlugins {
  #plugins: PluginSnapshot[] = PLUGINS.map(plugin => ({ ...plugin }))
  readonly #audit = new Map<string, AuditEntryDto[]>(Object.entries(PLUGIN_AUDIT).map(([id, rows]) => [id, [...rows]]))
  constructor(private emit: (type: SequencedKernelEvent['type'], payload: unknown) => void) {
    const base = this.#plugins[0]!
    this.#plugins.push(
      { ...base, pluginId: '@preview/notes', title: 'Notes', description: 'Browse and organize your notes.', hasUi: true },
      { ...base, pluginId: '@preview/calendar', title: 'Calendar', enabled: false, state: 'disabled', hasUi: true },
      { ...base, pluginId: '@preview/archive', title: 'Archive with a deliberately long descriptive name', state: 'awaiting_consent', granted: [], denied: [], requested: ['fs:read', 'net:fetch:archive.example.test'] }
    )
  }

  invoke(command: string, args: Readonly<Record<string, unknown>> = {}): unknown {
    const id = String(args.pluginId)
    const plugin = this.#plugins.find(plugin => plugin.pluginId === id)
    switch (command) {
      case 'plugins_list': return this.#plugins.map(plugin => ({ ...plugin }))
      case 'plugins_publishers': return []
      case 'plugins_publisher_decide': return null
      case 'plugins_enable':
      case 'plugins_disable':
      case 'plugins_reload':
      case 'plugins_consent_respond': {
        if (!plugin)
          return { code: 'not_found', message: 'error.notFound', retryable: false }
        const granted = command === 'plugins_consent_respond' ? args.granted as string[] : plugin.granted
        const enabled = command === 'plugins_enable' || (command !== 'plugins_disable' && plugin.enabled)
        const next: PluginSnapshot = { ...plugin, enabled, granted, denied: (plugin.requested ?? []).filter(capability => !granted.includes(capability)), state: enabled ? granted.length ? 'active' : 'awaiting_consent' : 'disabled' }
        this.#plugins = this.#plugins.map(item => item.pluginId === id ? next : item)
        this.emit('pluginUpdated', next)
        return command === 'plugins_consent_respond' || command === 'plugins_reload' ? next : null
      }
      case 'plugins_catalog_search': {
        const query = args.query as { query?: string, offset?: number }
        const all: CatalogItem[] = this.#plugins.map(plugin => ({ pluginId: plugin.pluginId, title: plugin.title, description: plugin.description ?? '', version: plugin.version, publisher: plugin.publisher, capabilityCount: plugin.requested?.length ?? 0, installed: true }))
        all.push({ pluginId: '@preview/reader', title: 'Reader', description: 'Read saved articles on your devices.', version: '1.0.0', publisher: '@preview', capabilityCount: 2, installed: this.#plugins.some(plugin => plugin.pluginId === '@preview/reader') })
        return all.filter(item => (item.title + item.pluginId).toLowerCase().includes((query.query ?? '').toLowerCase())).slice(query.offset ?? 0, (query.offset ?? 0) + 20)
      }
      case 'plugins_request_registry_search': return []
      case 'plugins_preview':
        return { pluginId: id, title: 'Reader', version: '1.0.0', description: 'Read saved articles on your devices.', publisher: '@preview', hasUi: true, headless: true, capabilities: ['fs:read', 'net:fetch:reader.example.test'] } satisfies InstallPreviewDto
      case 'plugins_install': {
        const source = args.source as { source: string, pluginId?: string }
        const pluginId = source.pluginId ?? '@preview/local'
        const next = { ...this.#plugins[0]!, pluginId, title: source.source === 'registry' ? 'Reader' : 'Local plugin', enabled: false, state: 'disabled', granted: [], denied: [], requested: ['fs:read'], hasUi: true }
        this.#plugins = [...this.#plugins.filter(plugin => plugin.pluginId !== pluginId), next]
        this.emit('pluginUpdated', next)
        return { pluginId, version: next.version, enabled: false } satisfies InstallReport
      }
      case 'plugins_uninstall':
        this.#plugins = this.#plugins.filter(plugin => plugin.pluginId !== id)
        return { bytesFreed: 0, dataKept: args.removeData !== true, leftovers: [] }
      case 'plugins_updates': return []
      case 'plugins_check_update': return null
      case 'plugins_update': return { pluginId: id, version: '1.2.0', enabled: false, warnings: [] } satisfies InstallReport
      case 'plugins_audit': return (this.#audit.get(id) ?? []).slice(Number(args.offset ?? 0), Number(args.offset ?? 0) + Number(args.limit ?? 20))
      case 'plugins_audit_clear':
        this.#audit.delete(id)
        return null
      case 'plugins_sync_plan': return { pluginId: id, targets: DEVICES.filter(device => device.paired).map(device => ({ deviceId: device.deviceId, displayName: device.displayName, online: device.online, needsArtifact: true })) } satisfies SyncPlanDto
      case 'plugins_sync_run': {
        const plan = args.plan as SyncPlanDto
        return { updated: plan.targets.filter(target => target.online && target.needsArtifact).length, unchanged: plan.targets.filter(target => !target.needsArtifact).length, failures: plan.targets.filter(target => !target.online && target.needsArtifact).map(target => ({ deviceId: target.deviceId, error: { code: 'unavailable', message: 'error.unavailable', retryable: true } })) } satisfies SyncReport
      }
      default: return { code: 'unsupported', message: 'error.commandNotFound', retryable: false }
    }
  }
}
