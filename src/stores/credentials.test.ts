import type { CredentialSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'

import { DEVICES, IDENTITY } from '../ipc/fake-kernel-fixtures'
import { useCredentialsStore } from './credentials'
import { useDevicesStore } from './devices'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

const initial: CredentialSnapshot = {
  deviceId: IDENTITY.deviceId,
  enrollment: { fingerprint: 'old', backend: 'windows', ready: true },
  retiredKeys: 0,
  runtimeReady: true,
  restartRequired: false
}

beforeEach(() => {
  setActivePinia(createPinia())
  call.mockReset()
})

it('discards a read invalidated by successful repair and retains the independent cleanup error', async () => {
  let finishOld!: (value: unknown) => void
  let reads = 0
  call.mockImplementation(async (command: string) => {
    if (command === 'identity_credentials') {
      reads += 1
      if (reads === 1) {
        return new Promise((resolve) => {
          finishOld = resolve
        })
      }
      return { ok: true, value: { ...initial, enrollment: { ...initial.enrollment, fingerprint: 'new' }, runtimeReady: false, restartRequired: true, retiredKeys: 1 } }
    }
    if (command === 'identity_repair') {
      return { ok: true, value: { deviceId: IDENTITY.deviceId, fingerprint: 'new', restartRequired: true, cleanup: { remaining: null, errorCode: 'internal', detail: 'cleanup failed' } } }
    }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [] } }
    throw new Error(command)
  })
  const store = useCredentialsStore()
  const reading = store.refresh()
  const repairing = store.repair('old')
  await Promise.resolve()
  finishOld({ ok: true, value: initial })
  await Promise.all([reading, repairing])
  expect(store.snapshot?.enrollment?.fingerprint).toBe('new')
  expect(store.restartRequired).toBe(true)
  expect(store.repaired?.fingerprint).toBe('new')
  expect(store.cleanup?.errorCode).toBe('internal')
  expect(call).toHaveBeenCalledWith('identity_repair', { expectedFingerprint: 'old' })
})

it('re-reads revoked metadata and the changed enrollment even after a failed replacement', async () => {
  const devices = useDevicesStore()
  devices.applySnapshot({ identity: IDENTITY, devices: DEVICES })
  call.mockImplementation(async (command: string) => {
    if (command === 'identity_repair')
      return { ok: false, error: { code: 'internal', message: 'error.internal' } }
    if (command === 'identity_credentials')
      return { ok: true, value: { ...initial, enrollment: { fingerprint: 'pending', backend: 'windows', ready: false }, runtimeReady: false } }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [] } }
    throw new Error(command)
  })
  const store = useCredentialsStore()
  expect(await store.repair('old')).toBe(false)
  expect(store.error?.code).toBe('internal')
  expect(store.snapshot?.enrollment?.ready).toBe(false)
  expect(store.snapshot?.enrollment?.fingerprint).toBe('pending')
  expect(devices.cards).toEqual([])
  expect(store.repaired).toBeNull()
})

it('reports a failed device-list refresh independently and retries it without replacing the key again', async () => {
  let reads = 0
  call.mockImplementation(async (command: string) => {
    if (command === 'identity_repair') {
      return { ok: true, value: { deviceId: IDENTITY.deviceId, fingerprint: 'new', restartRequired: true, cleanup: { remaining: 0, errorCode: null, detail: null } } }
    }
    if (command === 'identity_credentials')
      return { ok: true, value: initial }
    if (command === 'devices_snapshot') {
      reads += 1
      return reads === 1
        ? { ok: false, error: { code: 'internal', message: 'error.internal' } }
        : { ok: true, value: { identity: IDENTITY, devices: [] } }
    }
    throw new Error(command)
  })
  const store = useCredentialsStore()
  await store.repair('old')
  expect(store.repaired?.fingerprint).toBe('new')
  expect(store.error?.code).toBe('internal')
  await store.reload()
  expect(store.error).toBeUndefined()
  expect(call.mock.calls.filter(([command]) => command === 'identity_repair')).toHaveLength(1)
})
