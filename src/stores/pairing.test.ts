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
    if (command === 'pairing_cancel')
      return { ok: true, value: null }
    throw new Error(command)
  })
  const store = usePairingStore()
  const first = store.refresh()
  const invalidation = store.refresh()
  const command = store.cancel()
  await Promise.resolve()
  resolveOld({ ok: true, value: task })
  await Promise.all([first, invalidation, command])
  expect(store.snapshot?.phase).toBe('paired')
  expect(store.active).toBe(false)
  expect(reads).toBe(2)
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

it('serializes cancellation until the backend has answered', async () => {
  let finish!: (answer: unknown) => void
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_status')
      return { ok: true, value: task }
    if (command === 'pairing_cancel') {
      return new Promise((resolve) => {
        finish = resolve
      })
    }
    throw new Error(command)
  })
  const store = usePairingStore()
  await store.refresh()
  const first = store.cancel()
  expect(await store.cancel()).toBe(false)
  expect(store.active).toBe(true)
  expect(call.mock.calls.filter(([command]) => command === 'pairing_cancel')).toHaveLength(1)
  call.mockResolvedValue({ ok: true, value: { ...task, phase: 'cancelled' } })
  finish({ ok: true, value: null })
  expect(await first).toBe(true)
  expect(store.snapshot?.phase).toBe('cancelled')
})
