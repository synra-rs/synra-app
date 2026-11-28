import { APP_PROFILE, createMockClient, synraError } from '@synra/testkit'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import { decodeTransferStatus } from '@/ipc/injection/transfer-status'
import fixture from '../fixtures/plugin-event-contract.json'
import { context, streamFixture } from './injection-test-helpers'

describe('plugin page transfer events', () => {
  it('merges a real failed transfer event ahead of the initial query and limits callbacks to the selected resource', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const status = fixture.transfer.payload
    const peer = vi.fn()
    const seen = vi.fn()
    let reply: (value: unknown) => void = () => {}
    stream.status.mockImplementation(() => new Promise((resolve) => {
      reply = resolve
    }))
    const off = app.transfer.onProgress(status.transferId, seen)
    const stopBroad = app.event.subscribe('*.*', peer)
    expect(stream.invoke).toHaveBeenNthCalledWith(1, 'plugin_ui_rpc', expect.objectContaining({
      method: 'event.subscribe',
      args: { transferId: status.transferId, subscriptionId: 1 }
    }))
    expect(stream.status).not.toHaveBeenCalled()
    await stream.push({
      ...fixture.transfer,
      seq: 1,
      payload: { ...status, transferId: 'another-owned-id' }
    })
    await vi.waitFor(() => expect(peer).toHaveBeenCalledTimes(1))
    expect(seen).not.toHaveBeenCalled()
    await stream.push({ ...fixture.transfer, seq: 2 })
    await vi.waitFor(() => expect(seen).toHaveBeenCalledWith({ ...status, ratio: 0.5 }))
    reply({ ...status, revision: 1, state: 'transferring' })
    await stream.push({ ...fixture.transfer, seq: 3 })
    await vi.waitFor(() => expect(peer).toHaveBeenCalledTimes(3))
    expect(seen).toHaveBeenCalledTimes(1)
    expect(stream.isOverlapping()).toBe(false)
    expect(stream.transport.listen).not.toHaveBeenCalled()
    off()
    stopBroad()
    await stream.push(null)
  })

  it('retries a timed-out initial transfer snapshot with the subscription still active', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const status = fixture.transfer.payload
    const seen = vi.fn()
    const onError = vi.fn()
    let reply: (value: unknown) => void = () => {}
    stream.status.mockImplementationOnce(() => {
      throw synraError('timeout', 'error.timeout')
    })
    stream.status.mockImplementation(() => new Promise((resolve) => {
      reply = resolve
    }))
    const off = app.transfer.onProgress(status.transferId, seen, { onError })

    try {
      await vi.waitFor(() => expect(stream.status).toHaveBeenCalledTimes(2))
      await stream.push({ ...fixture.transfer, seq: 1 })
      await vi.waitFor(() => expect(seen).toHaveBeenCalledWith({ ...status, ratio: 0.5 }))
      reply({ ...status, revision: 1, state: 'transferring' })
      await stream.push({ ...fixture.transfer, seq: 2 })
      await vi.waitFor(() => {
        expect(stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.poll')).toHaveLength(3)
      })
      expect(seen).toHaveBeenCalledTimes(1)
      expect(onError).not.toHaveBeenCalled()
      expect(stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.unsubscribe')).toHaveLength(0)
    }
    finally {
      off()
      await stream.push(null)
    }
  })

  it('reports a persistently unavailable initial snapshot after bounded retries', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const refusal = synraError('timeout', 'error.timeout')
    stream.status.mockImplementation(() => {
      throw refusal
    })
    const seen = vi.fn()
    const onError = vi.fn()
    const off = app.transfer.onProgress(fixture.transfer.payload.transferId, seen, { onError })

    try {
      await vi.waitFor(() => expect(onError).toHaveBeenCalledExactlyOnceWith(refusal))
      expect(stream.status).toHaveBeenCalledTimes(3)
      expect(seen).not.toHaveBeenCalled()
      expect(stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.unsubscribe')).toHaveLength(1)
    }
    finally {
      off()
      await stream.push(null)
    }
  })

  it('recovers the retained terminal status after a missed host delivery and a retryable IPC error', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const mock = createMockClient({
      profile: APP_PROFILE,
      capabilities: ['file:transfer:send']
    })
    const status = decodeTransferStatus(fixture.transfer.payload)
    const initial = decodeTransferStatus({ ...status, state: 'transferring', revision: 1 })
    stream.status.mockReturnValue(initial)
    stream.refresh.mockReturnValue(fixture.recovery)
    const seen = vi.fn()
    const mockSeen = vi.fn()
    const onError = vi.fn()
    mock.setTransferStatus(initial)
    const stopMock = mock.transfer.onProgress(status.transferId, mockSeen)
    const off = app.transfer.onProgress(status.transferId, seen, { onError })
    await vi.waitFor(() => expect(seen).toHaveBeenCalledWith(expect.objectContaining({ state: 'transferring' })))
    await stream.fail(synraError('timeout', 'error.timeout'))
    mock.reportProgress({ ...status, ratio: 0.5 })
    await vi.waitFor(() => expect(seen).toHaveBeenLastCalledWith({ ...status, ratio: 0.5 }))
    await stream.push(fixture.resync)
    await vi.waitFor(() => expect(stream.refresh).toHaveBeenCalledTimes(2))
    expect(seen).toHaveBeenCalledTimes(2)
    expect(seen.mock.calls).toEqual(mockSeen.mock.calls)
    stream.status.mockReturnValue(status)
    expect(await app.transfer.status(status.transferId)).toEqual(await mock.transfer.status(status.transferId))
    expect(onError).not.toHaveBeenCalled()
    stopMock()
    off()
    await stream.push(null)
  })

  it('ignores a transfer status query that resolves after page disposal', async () => {
    const stream = streamFixture()
    const lifetime = new EventTarget()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }), lifetime)
    const seen = vi.fn()
    let reply: (value: unknown) => void = () => {}
    stream.status.mockImplementation(() => new Promise((resolve) => {
      reply = resolve
    }))
    app.transfer.onProgress(fixture.transfer.payload.transferId, seen)
    await vi.waitFor(() => expect(stream.status).toHaveBeenCalledTimes(1))
    lifetime.dispatchEvent(new Event('pagehide'))
    reply(fixture.transfer.payload)
    await stream.push(null)
    expect(seen).not.toHaveBeenCalled()
  })
})
