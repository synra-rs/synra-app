import type { PluginDevicePlatform, PluginDeviceTarget, PluginEventEnvelope } from '@synra/protocol'
import { createSynraError } from '../../errors'
import { data } from '../../errors/shape'

export const DEVICE_TOPIC = 'device.changed' satisfies PluginEventEnvelope['name']

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

export function decodeEnvelope(raw: unknown): PluginEventEnvelope {
  const name = data(raw, 'name')
  const seq = data(raw, 'seq')
  const at = data(raw, 'at')
  if (name !== DEVICE_TOPIC || typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 1
    || typeof at !== 'number' || !Number.isSafeInteger(at)) {
    return invalid('invalid plugin event envelope')
  }
  return { name, seq, at, payload: { devices: decodeDevices(data(data(raw, 'payload'), 'devices')) } }
}
