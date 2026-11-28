import { synraError } from '@synra/testkit'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import fixture from '../fixtures/plugin-event-contract.json'
import { context, streamFixture } from './injection-test-helpers'

describe('the first transfer status from an active subscription', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it.each([
    {
      source: 'live delivery before rejection',
      cursor: 1,
      resync: false,
      alreadyWaiting: false
    },
    {
      source: 'recovery before rejection',
      cursor: 7,
      resync: true,
      alreadyWaiting: false
    },
    {
      source: 'live delivery during retry delay',
      cursor: 1,
      resync: false,
      alreadyWaiting: true
    }
  ])('keeps receiving after $source supplies the initial state', async ({ cursor, resync, alreadyWaiting }) => {
    vi.useFakeTimers()
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const terminal = fixture.transfer.payload
    const current = {
      transferId: terminal.transferId,
      state: 'transferring',
      revision: terminal.revision - 1,
      receivedBytes: terminal.receivedBytes,
      totalBytes: terminal.totalBytes,
      blocksDone: terminal.blocksDone,
      blocksTotal: terminal.blocksTotal
    }
    const refusal = synraError('timeout', 'error.timeout')
    let rejectInitial: (reason: unknown) => void = () => {}
    stream.status.mockImplementationOnce(() => new Promise((_, reject) => {
      rejectInitial = reject
    })).mockRejectedValue(refusal)
    stream.refresh.mockReturnValue({
      seq: cursor,
      events: [
        { name: fixture.transfer.name, payload: current }
      ]
    })
    const seen = vi.fn()
    const onError = vi.fn()
    const off = app.transfer.onProgress(terminal.transferId, seen, { onError })

    try {
      await vi.waitFor(() => expect(stream.status).toHaveBeenCalledOnce())
      if (alreadyWaiting) {
        rejectInitial(refusal)
        await vi.advanceTimersByTimeAsync(0)
        expect(stream.status).toHaveBeenCalledOnce()
      }
      await stream.push(resync
        ? fixture.resync
        : { ...fixture.transfer, seq: cursor, payload: current })
      await vi.advanceTimersByTimeAsync(0)
      expect(seen).toHaveBeenCalledExactlyOnceWith({ ...current, ratio: 0.5 })

      if (!alreadyWaiting)
        rejectInitial(refusal)
      await vi.advanceTimersByTimeAsync(1000)
      expect(stream.status).toHaveBeenCalledOnce()
      expect(onError).not.toHaveBeenCalled()

      await stream.push({ ...fixture.transfer, seq: cursor + 1 })
      await vi.waitFor(() => expect(seen).toHaveBeenLastCalledWith({ ...terminal, ratio: 0.5 }))
      expect(seen).toHaveBeenCalledTimes(2)
      expect(stream.isOverlapping()).toBe(false)
    }
    finally {
      off()
      await stream.push(null)
    }
  })

  it('skips failure callbacks for a subscription unsubscribed by another handler', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['file:transfer:send'],
      bridge: stream.transport
    }))
    const { transferId } = fixture.transfer.payload
    let stopSecond = () => {}
    const onFirstError = vi.fn(() => stopSecond())
    const onSecondError = vi.fn()
    const stopFirst = app.transfer.onProgress(transferId, vi.fn(), { onError: onFirstError })
    stopSecond = app.transfer.onProgress(transferId, vi.fn(), { onError: onSecondError })

    try {
      await vi.waitFor(() => expect(stream.status).toHaveBeenCalledTimes(2))
      const refusal = synraError('protocol_violation', 'error.protocol_violation')
      await stream.fail(refusal)
      await vi.waitFor(() => expect(onFirstError).toHaveBeenCalledExactlyOnceWith(refusal))
      expect(onSecondError).not.toHaveBeenCalled()
      expect(stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.unsubscribe')).toHaveLength(2)
    }
    finally {
      stopFirst()
      stopSecond()
    }
  })
})
