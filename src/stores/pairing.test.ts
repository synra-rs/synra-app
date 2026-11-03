import type { PairingSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { usePairingStore } from './pairing'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

const peer = { deviceId: 'peer', displayName: '安卓', platform: 'android', code: '000123' }
const waiting: PairingSnapshot = { phase: 'waiting', offer: peer }
const asked: PairingSnapshot = { phase: 'asked', ask: peer }

beforeEach(() => {
  setActivePinia(createPinia())
  call.mockReset()
})

describe('pairing across the application', () => {
  it('keeps the code and peer while waiting and keeps the name after completion', () => {
    const store = usePairingStore()
    store.apply({ type: 'pairingUpdated', payload: waiting })
    expect(store.visible).toBe(true)
    expect(store.snapshot.offer?.code).toBe('000123')
    expect(store.snapshot.outcome).toBeUndefined()
    store.apply({ type: 'pairingUpdated', payload: { phase: 'done', outcome: 'paired' } })
    expect(store.peerName).toBe('安卓')
    expect(store.snapshot.outcome).toBe('paired')
  })

  it('does not overwrite a completion event with an earlier command response', async () => {
    const store = usePairingStore()
    call.mockImplementation(async () => {
      store.apply({ type: 'pairingUpdated', payload: { phase: 'done', outcome: 'refused' } })
      return { ok: true, value: waiting }
    })
    await store.start({ ...peer, paired: false, online: true, blocked: false, lastSeenMs: 0 })
    expect(store.snapshot.phase).toBe('done')
    expect(store.snapshot.outcome).toBe('refused')
  })

  it('does not overwrite a newly received request with a late initial snapshot', async () => {
    const store = usePairingStore()
    call.mockImplementation(async () => {
      store.apply({ type: 'pairingUpdated', payload: asked })
      return { ok: true, value: { phase: 'idle' } }
    })
    await store.read()
    expect(store.snapshot).toEqual(asked)
    expect(store.visible).toBe(true)
  })

  it('persists a block as a refusal with the block flag', async () => {
    const store = usePairingStore()
    store.apply({ type: 'pairingUpdated', payload: asked })
    call.mockResolvedValue({ ok: true, value: { phase: 'done', outcome: 'refused' } })
    await store.respond(false, true)
    expect(call).toHaveBeenCalledWith('devices_pairing_respond', { accepted: false, block: true })
    expect(store.snapshot.outcome).toBe('refused')
  })

  it('keeps a failed refusal visible and does not silently dismiss the other device', async () => {
    const store = usePairingStore()
    store.apply({ type: 'pairingUpdated', payload: asked })
    call.mockResolvedValue({ ok: false, error: { code: 'internal', message: 'errors.internal', retryable: false } })
    await store.close()
    expect(store.visible).toBe(true)
    expect(store.snapshot.phase).toBe('asked')
    expect(store.error?.code).toBe('internal')
  })

  it('withdraws on close and stays dismissed when the worker repeats its final event', async () => {
    const store = usePairingStore()
    store.apply({ type: 'pairingUpdated', payload: waiting })
    call.mockResolvedValue({ ok: true, value: { phase: 'done', outcome: 'cancelled' } })
    await store.close()
    expect(call).toHaveBeenCalledWith('devices_pairing_cancel')
    store.apply({ type: 'pairingUpdated', payload: { phase: 'done', outcome: 'cancelled' } })
    expect(store.visible).toBe(false)
  })
})
