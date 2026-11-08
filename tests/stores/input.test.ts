import type { InputSession } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'
import { useInputStore } from '@/stores/input'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('@/app/kernel', () => ({ kernel: { call } }))
const session: InputSession = { sessionId: 'session', deviceId: 'phone', displayName: '安卓', pluginId: '@synra-plugin/touchpad', phase: 'pending' }
beforeEach(() => {
  setActivePinia(createPinia())
  call.mockReset()
})

it('retains a newer consent event when an old initial snapshot arrives', async () => {
  const store = useInputStore()
  call.mockImplementation(async () => {
    store.apply({ type: 'inputUpdated', payload: { session } })
    return { ok: true, value: {} }
  })
  await store.read()
  expect(store.snapshot.session).toEqual(session)
})

it('does not invent acceptance from a successful decision command', async () => {
  const store = useInputStore()
  store.apply({ type: 'inputUpdated', payload: { session } })
  call.mockResolvedValue({ ok: true, value: null })
  await store.decide(true)
  expect(call).toHaveBeenCalledWith('devices_input_respond', { sessionId: 'session', allow: true })
  expect(store.snapshot.session?.phase).toBe('pending')
  store.apply({ type: 'inputUpdated', payload: { session: { ...session, phase: 'active' } } })
  await store.stop()
  expect(call).toHaveBeenLastCalledWith('devices_input_stop', { sessionId: 'session' })
})

it('keeps the newest reread when concurrent snapshots finish in reverse order', async () => {
  const store = useInputStore()
  const pending: Array<(value: unknown) => void> = []
  call.mockImplementation(() => new Promise(resolve => pending.push(resolve)))
  const first = store.read()
  const second = store.read()
  pending[1]!({ ok: true, value: { session: { ...session, phase: 'ended' } } })
  await second
  pending[0]!({ ok: true, value: { session } })
  await first
  expect(store.snapshot.session?.phase).toBe('ended')
})

it('keeps failed refusals visible and ignores failures for superseded requests', async () => {
  const store = useInputStore()
  store.apply({ type: 'inputUpdated', payload: { session } })
  const error = { code: 'internal', message: 'errors.internal', retryable: false }
  call.mockResolvedValue({ ok: false, error })
  await store.decide(false)
  expect(store.error).toEqual(error)
  call.mockImplementation(async () => {
    store.apply({ type: 'inputUpdated', payload: { session: { ...session, sessionId: 'new' } } })
    return { ok: false, error }
  })
  await store.decide(false)
  expect(store.error).toBeUndefined()
})
