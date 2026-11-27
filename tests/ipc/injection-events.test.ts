import { APP_PROFILE, createMockClient, matches, synraError } from '@synra/testkit'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import { compileEventPattern } from '@/ipc/injection/event/pattern'
import fixture from '../fixtures/plugin-event-contract.json'
import { bridge, context } from './injection-test-helpers'

function streamFixture() {
  const reads: ((value: unknown) => void)[] = []
  let overlapping = false
  const refresh = vi.fn<() => unknown>(() => null)
  const invoke = vi.fn(async (command: string, request?: Readonly<Record<string, unknown>>) => {
    expect(command).toBe('plugin_ui_rpc')
    switch (request?.method) {
      case 'event.subscribe': return 'stream'
      case 'event.poll': return new Promise((resolve) => {
        if (reads.length !== 0)
          overlapping = true
        reads.push(resolve)
      })
      case 'event.resync': return refresh()
      case 'event.unsubscribe': return null
      case 'device.list': return fixture.envelope.payload.devices
      default: throw new Error('unexpected event command')
    }
  })
  const transport = bridge({ invoke })
  return {
    transport,
    invoke,
    refresh,
    async push(frame: unknown) {
      await vi.waitFor(() => expect(reads).toHaveLength(1))
      const read = reads.shift()
      read?.(frame)
    },
    isOverlapping: () => overlapping
  }
}

describe('patterns produced by Rust', () => {
  it.each(fixture.patterns)('$pattern receives $name only when Core matches', (vector) => {
    const compiled = compileEventPattern(vector.pattern)
    expect(compiled !== undefined).toBe(vector.valid)
    expect(compiled?.(vector.name) ?? false).toBe(vector.matches)
    expect(matches(vector.pattern, vector.name)).toBe(vector.matches)
  })
})

describe('plugin page event stream', () => {
  it('delivers only matching public payloads through one receive and detaches each subscription once', async () => {
    const stream = streamFixture()
    const lifetime = new EventTarget()
    const ctx = context({ granted: ['device:list'], bridge: stream.transport })
    const app = createInjection(ctx, lifetime)
    const devicesChanged = vi.fn()
    const wildcard = vi.fn()
    const other = vi.fn()
    const off = app.event.subscribe('device.*', wildcard)
    app.event.subscribe('device.future', other)
    app.device.onChange(devicesChanged)
    await stream.push(fixture.envelope)
    await vi.waitFor(() => expect(wildcard).toHaveBeenCalledWith(fixture.envelope.payload))
    expect(devicesChanged).toHaveBeenCalledWith(await app.device.list())
    expect(other).not.toHaveBeenCalled()
    off()
    off()
    await stream.push({ ...fixture.envelope, seq: 2 })
    await vi.waitFor(() => expect(devicesChanged).toHaveBeenCalledTimes(2))
    expect(wildcard).toHaveBeenCalledTimes(1)

    lifetime.dispatchEvent(new Event('pagehide'))
    await stream.push({ ...fixture.envelope, seq: 3 })
    await vi.waitFor(() => {
      const stops = stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.unsubscribe')
      expect(stops).toHaveLength(3)
      expect(devicesChanged).toHaveBeenCalledTimes(2)
    })
    expect(stream.isOverlapping()).toBe(false)
    expect(stream.transport.listen).not.toHaveBeenCalled()
  })

  it('re-reads the authority snapshot on a gap, cancels stale replay and drops duplicate sequences', async () => {
    const stream = streamFixture()
    const client = createInjection(context({ granted: ['device:list'], bridge: stream.transport }))
    const seen = vi.fn()
    const off = client.device.onChange(seen)
    await stream.push(fixture.envelope)
    await vi.waitFor(() => expect(seen).toHaveBeenCalledTimes(1))
    await stream.push(fixture.envelope)
    const newer = { devices: [] }
    stream.refresh.mockReturnValue({ ...fixture.envelope, seq: 5, payload: newer })
    await stream.push({ ...fixture.envelope, seq: 3 })
    await vi.waitFor(() => expect(seen).toHaveBeenLastCalledWith([]))
    expect(seen).toHaveBeenCalledTimes(2)
    expect(stream.refresh).toHaveBeenCalledTimes(1)

    await stream.push({ ...fixture.envelope, seq: 4 })
    await stream.push({ ...fixture.envelope, seq: 6 })
    await vi.waitFor(() => expect(seen).toHaveBeenCalledTimes(3))
    off()
    await stream.push(null)
  })

  it('rejects a management envelope and reports the failure without calling a subscriber', async () => {
    const stream = streamFixture()
    const client = createInjection(context({ granted: ['device:list'], bridge: stream.transport }))
    const callback = vi.fn()
    const onError = vi.fn()
    client.event.subscribe('device.changed', callback, { onError })
    await stream.push({ seq: 1, at: 20, type: 'deviceCardUpdated', payload: { address: 'private' } })
    await vi.waitFor(() => expect(onError).toHaveBeenCalledTimes(1))
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({ code: 'protocol_violation' }))
    expect(callback).not.toHaveBeenCalled()
    expect(stream.invoke.mock.calls.filter(([, args]) => args?.method === 'event.unsubscribe')).toHaveLength(1)
  })

  it('reports a stale session rejection during registration', async () => {
    const denial = synraError('capability_denied', 'error.capability_denied')
    const invoke = vi.fn(async () => {
      throw denial
    })
    const client = createInjection(context({ granted: ['device:list'], bridge: bridge({ invoke }) }))
    const handler = vi.fn()
    const onError = vi.fn()
    const off = client.device.onChange(handler, { onError })
    await vi.waitFor(() => expect(onError).toHaveBeenCalledWith(denial))
    off()
    expect(handler).not.toHaveBeenCalled()
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('keeps testkit and App device list and event delivery on the same Rust fixture', async () => {
    const stream = streamFixture()
    const app = createInjection(context({ granted: ['device:list'], bridge: stream.transport }))
    const mock = createMockClient({ profile: APP_PROFILE, capabilities: ['device:list'] })
    const appSeen = vi.fn()
    const mockSeen = vi.fn()
    const off = app.device.onChange(appSeen)
    const stopMock = mock.device.onChange(mockSeen)
    const offTopic = mock.event.subscribe('device.*', vi.fn())
    const devices = await app.device.list()
    mock.setDevices(devices)
    await stream.push(fixture.envelope)
    await vi.waitFor(() => expect(appSeen.mock.calls).toEqual(mockSeen.mock.calls))
    expect(await mock.device.list()).toEqual(devices)
    stopMock()
    offTopic()
    off()
    await stream.push(null)
  })
})
