import type { PluginDevicePlatform, PluginDeviceTarget, PluginEvent, PluginEventPoll, PluginEventRecovery } from '@synra/protocol'
import { createSynraError } from '../../errors'
import { data } from '../../errors/shape'
import { decodeTransferStatus } from '../transfer-status'

export const DEVICE_TOPIC = 'device.changed' satisfies PluginEvent['name']
export const TRANSFER_TOPIC = 'transfer.progress' satisfies PluginEvent['name']

const PLATFORMS: ReadonlySet<string> = new Set<PluginDevicePlatform>([
  'windows',
  'macos',
  'linux',
  'android',
  'ios',
  'desktop',
  'headless'
])

function invalid(reason: string): never {
  throw createSynraError('protocol_violation', { reason })
}

function isPlatform(platform: unknown): platform is PluginDevicePlatform {
  return typeof platform === 'string' && PLATFORMS.has(platform)
}

export function decodeDevices(raw: unknown): PluginDeviceTarget[] {
  if (!Array.isArray(raw))
    return invalid('expected a plugin device collection')
  return raw.map((target: unknown) => {
    const deviceId = data(target, 'deviceId')
    const displayName = data(target, 'displayName')
    const platform = data(target, 'platform')
    const online = data(target, 'online')
    if (typeof deviceId !== 'string' || typeof displayName !== 'string' || !isPlatform(platform) || typeof online !== 'boolean')
      return invalid('invalid plugin device target')
    return { deviceId, displayName, platform, online }
  })
}

function decodeEvent(raw: unknown): PluginEvent {
  const name = data(raw, 'name')
  const payload = data(raw, 'payload')
  if (name === DEVICE_TOPIC)
    return { name, payload: { devices: decodeDevices(data(payload, 'devices')) } }
  if (name === TRANSFER_TOPIC)
    return { name, payload: decodeTransferStatus(payload) }
  return invalid('invalid plugin event topic')
}

export function decodePoll(raw: unknown): PluginEventPoll {
  if (data(raw, 'resync') === true)
    return { resync: true }
  const seq = data(raw, 'seq')
  const at = data(raw, 'at')
  if (typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 1
    || typeof at !== 'number' || !Number.isSafeInteger(at)) {
    return invalid('invalid plugin event envelope')
  }
  return { ...decodeEvent(raw), seq, at }
}

export function decodeRecovery(raw: unknown): PluginEventRecovery {
  const seq = data(raw, 'seq')
  const events = data(raw, 'events')
  if (typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 0 || !Array.isArray(events))
    return invalid('invalid plugin event recovery')
  return { seq, events: events.map(decodeEvent) }
}
