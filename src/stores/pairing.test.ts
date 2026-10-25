import type { PairingSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'

import { usePairingStore } from './pairing'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

const task: PairingSnapshot = {
  requestId: 'task-1',
  peerId: 'peer',
  phase: 'listening',
  expiresAtMs: 1000,
  reply: 'reply',
  errorCode: null,
  detail: null
}

beforeEach(() => {
  setActivePinia(createPinia())
  call.mockReset()
})

it('discards an old status read after a command and coalesces invalidations', async () => {
  let resolveOld!: (value: unknown) => void
  let reads = 0
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_status') {
      reads += 1
      if (reads === 1) {
        return new Promise((resolve) => {
          resolveOld = resolve
        })
      }
      return { ok: true, value: { ...task, phase: 'paired' } }
    }
    if (command === 'pairing_connect')
      return { ok: true, value: { ...task, phase: 'connecting' } }
    throw new Error(command)
  })
  const store = usePairingStore()
  const first = store.refresh()
  const invalidation = store.refresh()
  const connected = store.connect('peer', ' imported reply ')
  await Promise.resolve()
  resolveOld({ ok: true, value: task })
  await Promise.all([first, invalidation, connected])
  expect(store.snapshot?.phase).toBe('paired')
  expect(store.active).toBe(false)
  expect(reads).toBe(2)
  expect(call).toHaveBeenCalledWith('pairing_connect', { deviceId: 'peer', reply: 'imported reply' })
})

it('cancels the backend generation and retains its terminal result across store consumers', async () => {
  let current = task
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_cancel') {
      current = { ...task, phase: 'cancelled' }
      return { ok: true, value: null }
    }
    if (command === 'pairing_status')
      return { ok: true, value: current }
    throw new Error(command)
  })
  const store = usePairingStore()
  await store.refresh()
  await store.cancel()
  expect(call).toHaveBeenCalledWith('pairing_cancel', { requestId: task.requestId })
  expect(usePairingStore().snapshot?.phase).toBe('cancelled')
  expect(store.active).toBe(false)
})

it('exposes refusals and allows a successful refresh to clear them', async () => {
  call.mockResolvedValueOnce({ ok: false, error: { code: 'not_paired', message: 'error.not_paired' } })
  const store = usePairingStore()
  expect(await store.refresh()).toBe(false)
  expect(store.error?.code).toBe('not_paired')
  call.mockResolvedValueOnce({ ok: true, value: null })
  expect(await store.refresh()).toBe(true)
  expect(store.error).toBeUndefined()
})

it('keeps an import refusal when a previously started status read succeeds', async () => {
  let finish!: (value: unknown) => void
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_status') {
      return new Promise((resolve) => {
        finish = resolve
      })
    }
    if (command === 'pairing_listen')
      return { ok: false, error: { code: 'not_paired', message: 'error.not_paired' } }
    throw new Error(command)
  })
  const store = usePairingStore()
  const reading = store.refresh()
  expect(await store.listen('peer', 'invalid certificate')).toBe(false)
  finish({ ok: true, value: null })
  await reading
  expect(store.error?.code).toBe('not_paired')
})
