/**
 * The fake refuses what the shell refuses: a call that is missing an argument its command declares.
 *
 * # The drift this exists to catch, because it happened
 *
 * `DevicesPage.vue` called `devices_scan_start` with no arguments at all. The shell's wrapper declares `opts: ScanOptions`,
 * and `#[tauri::command]` deserializes every argument *before* the command body runs — so the real bridge rejected the
 * invoke with `command devices_scan_start missing required key opts`, the client turned a rejection that is not an
 * `IpcError` into `error.bridgeFailed`, and the user read "The interface could not reach the kernel". No test noticed,
 * because the fake took no interest in its arguments and answered anyway.
 *
 * # Why the assertions are on the sentence
 *
 * The message is what makes the failure diagnosable: it names the command and the key, and it is the text
 * `tauri::Error::InvalidArgs` formats. Asserting on it rather than on "it threw" is what would have turned the user's
 * report into the line of source that was wrong.
 */

import type { DevicesSnapshot } from '@synra/protocol'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { KERNEL_EVENT_CHANNEL } from './channel'
import { KernelClient } from './client'
import { FakeKernel } from './fake-kernel'

afterEach(() => {
  vi.useRealTimers()
})

it.each([
  ['pairing_invite', undefined],
  ['pairing_listen', { deviceId: 'peer', invitation: 'invitation' }],
  ['pairing_connect', { deviceId: 'peer', reply: 'reply' }]
] as const)('reports %s as unavailable in preview through the real client', async (command, args) => {
  const result = await new KernelClient(new FakeKernel()).call(command, args)
  expect(result).toMatchObject({ ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
})

describe('a call that is missing an argument the shell requires', () => {
  it('is rejected rather than answered, in the shell\'s own words', async () => {
    await expect(new FakeKernel().invoke('devices_scan_start')).rejects.toThrow(
      'invalid args `opts` for command `devices_scan_start`: command devices_scan_start missing required key opts'
    )
  })

  it('is answered once the argument is there', async () => {
    // The answer is the scan's id — `One` is one value, and the shell returns a string.
    await expect(
      new FakeKernel().invoke('devices_scan_start', { opts: { includePaired: false, cachedOnly: true } })
    ).resolves.toBe('scan-cached')
  })

  it('rejects a key the command declares even when the fake ignores the value', async () => {
    // `devices_scan_cancel` never reads its identifier — it stops the timer. The *key* is still what the bridge
    // deserializes, so a caller that omits it reaches a command the shell never runs.
    await expect(new FakeKernel().invoke('devices_scan_cancel')).rejects.toThrow('missing required key scanId')
    await expect(new FakeKernel().invoke('devices_scan_cancel', { scanId: 'scan-1' })).resolves.toBeNull()
  })

  it('asks nothing of a command that takes nothing', async () => {
    await expect(new FakeKernel().invoke('devices_snapshot')).resolves.toBeTruthy()
  })
})

describe('the two scan modes', () => {
  it('exposes the running scan through the real client snapshot until completion or cancellation', async () => {
    vi.useFakeTimers()
    const client = new KernelClient(new FakeKernel())
    const started = await client.call<string>('devices_scan_start', { opts: { includePaired: false, cachedOnly: false } })
    expect(started.ok).toBe(true)
    if (!started.ok)
      throw new Error('scan start refused')
    const running = await client.call<DevicesSnapshot>('devices_snapshot')
    expect(running).toMatchObject({ ok: true, value: { scanning: { scanId: started.value, elapsedMs: 0 } } })
    vi.advanceTimersByTime(300)
    const progressed = await client.call<DevicesSnapshot>('devices_snapshot')
    expect(progressed).toMatchObject({ ok: true, value: { scanning: { scanId: started.value, elapsedMs: 300 } } })
    vi.advanceTimersByTime(600)
    const completed = await client.call<DevicesSnapshot>('devices_snapshot')
    expect(completed.ok && completed.value.scanning).toBeUndefined()
    const restarted = await client.call<string>('devices_scan_start', { opts: { includePaired: false, cachedOnly: false } })
    expect(restarted.ok).toBe(true)
    await client.call('devices_scan_cancel', { scanId: started.value })
    const cancelled = await client.call<DevicesSnapshot>('devices_snapshot')
    expect(cancelled.ok && cancelled.value.scanning).toBeUndefined()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('publishes nothing for a cached scan and reports a network scan as events', async () => {
    // ★ The fake used to scan whatever it was asked for, so the one mode this shell can answer looked exactly like the one
    // it cannot — in every test, and in `pnpm dev`. Both halves are asserted here, because "no events" only means
    // something beside a mode that does have them.
    vi.useFakeTimers()
    const kernel = new FakeKernel()
    const seen: unknown[] = []
    await kernel.listen(KERNEL_EVENT_CHANNEL, payload => seen.push(payload))

    await kernel.invoke('devices_scan_start', { opts: { includePaired: false, cachedOnly: true } })
    vi.advanceTimersByTime(2_000)
    expect(seen, 'a scan that is over before it answers has nothing to report').toEqual([])

    await kernel.invoke('devices_scan_start', { opts: { includePaired: false, cachedOnly: false } })
    vi.advanceTimersByTime(2_000)
    expect(seen.length, 'a network scan is what the events exist for').toBeGreaterThan(0)
  })

  it('stops the previous scan rather than leaving two timers running', async () => {
    vi.useFakeTimers()
    const kernel = new FakeKernel()
    const seen: unknown[] = []
    await kernel.listen(KERNEL_EVENT_CHANNEL, payload => seen.push(payload))

    await kernel.invoke('devices_scan_start', { opts: { includePaired: false, cachedOnly: false } })
    await kernel.invoke('devices_scan_start', { opts: { includePaired: false, cachedOnly: false } })
    // The two starts answer with an event each, and both happen at the same mocked instant — so a timer the first one left
    // behind would tick in step with the second one's, and one window would carry two events instead of one.
    seen.length = 0

    vi.advanceTimersByTime(300)
    expect(seen.length).toBe(1)
  })
})
