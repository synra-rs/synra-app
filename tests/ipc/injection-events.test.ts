import { APP_PROFILE, createMockClient, matches, synraError } from '@synra/testkit'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import { compileEventPattern } from '@/ipc/injection/event/pattern'
import fixture from '../fixtures/plugin-event-contract.json'
import { bridge, context, streamFixture } from './injection-test-helpers'

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
    const ctx = context({
      granted: ['device:list'],
      bridge: stream.transport
    })
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
    const client = createInjection(context({
      granted: ['device:list'],
      bridge: stream.transport
    }))
    const seen = vi.fn()
    const off = client.device.onChange(seen)
    await stream.push(fixture.envelope)
    await vi.waitFor(() => expect(seen).toHaveBeenCalledTimes(1))
    await stream.push(fixture.envelope)
    const newer = { devices: [] }
    stream.refresh.mockReturnValue({
      seq: 5,
      events: [
        { name: fixture.envelope.name, payload: newer }
      ]
    })
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
    const client = createInjection(context({
      granted: ['device:list'],
      bridge: stream.transport
    }))
    const callback = vi.fn()
    const onError = vi.fn()
    client.event.subscribe('device.changed', callback, { onError })
    await stream.push({
      seq: 1,
      at: 20,
      type: 'deviceCardUpdated',
      payload: { address: 'private' }
    })
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
    const client = createInjection(context({
      granted: ['device:list'],
      bridge: bridge({ invoke })
    }))
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
    const app = createInjection(context({
      granted: ['device:list'],
      bridge: stream.transport
    }))
    const mock = createMockClient({
      profile: APP_PROFILE,
      capabilities: ['device:list']
    })
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

  it('matches the same Rust custom event and enforces its concrete grant in App and testkit', async () => {
    const stream = streamFixture()
    const granted = [`event:subscribe:${fixture.custom.name}`]
    const app = createInjection(context({ granted, bridge: stream.transport }))
    const mock = createMockClient({ profile: APP_PROFILE, capabilities: granted })
    const appSeen = vi.fn()
    const mockSeen = vi.fn()
    const denied = '_plugin.@acme/other.update'
    expect(() => app.event.subscribe(denied, vi.fn())).toThrow(expect.objectContaining({ code: 'capability_denied' }))
    expect(() => mock.event.subscribe(denied, vi.fn())).toThrow(expect.objectContaining({ code: 'capability_denied' }))
    const off = app.event.subscribe('_plugin.*.*', appSeen)
    const stopMock = mock.event.subscribe('_plugin.*.*', mockSeen)
    await stream.push({ ...fixture.custom, seq: 1, name: denied })
    expect(mock.deliver(denied, fixture.custom.payload)).toBe(0)
    await stream.push({ ...fixture.custom, seq: 2 })
    expect(mock.deliver(fixture.custom.name, fixture.custom.payload)).toBe(1)
    await vi.waitFor(() => expect(appSeen.mock.calls).toEqual(mockSeen.mock.calls))
    expect(appSeen).toHaveBeenCalledWith(fixture.custom.payload)
    off()
    stopMock()
    await stream.push(null)
  })

  it('reports unreplayed occurrences to the indicated subscription and continues the stream', async () => {
    const stream = streamFixture()
    const app = createInjection(context({
      granted: ['device:list', `event:subscribe:${fixture.custom.name}`],
      bridge: stream.transport
    }))
    stream.refresh.mockReturnValue(fixture.customRecovery)
    const onGap = vi.fn()
    const onDeviceGap = vi.fn()
    const device = vi.fn()
    const custom = vi.fn()
    const stopDevice = app.event.subscribe('device.changed', device, { onGap: onDeviceGap })
    const stopCustom = app.event.subscribe(fixture.custom.name, custom, { onGap })
    await stream.push(fixture.envelope)
    await vi.waitFor(() => expect(device).toHaveBeenCalledOnce())
    await stream.push(fixture.custom)
    await vi.waitFor(() => expect(onGap).toHaveBeenCalledWith({ afterSeq: 1, throughSeq: 8 }))
    expect(onDeviceGap).not.toHaveBeenCalled()
    expect(custom).not.toHaveBeenCalled()
    expect(stream.invoke).toHaveBeenCalledWith('plugin_ui_rpc', expect.objectContaining({
      method: 'event.resync',
      args: { streamId: 'stream', afterSeq: 1 }
    }))
    await stream.push({ ...fixture.custom, seq: 9 })
    await vi.waitFor(() => expect(custom).toHaveBeenCalledWith(fixture.custom.payload))
    stopDevice()
    stopCustom()
    await stream.push(null)
  })
})
