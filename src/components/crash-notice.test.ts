// @vitest-environment jsdom
/**
 * The startup crash notice, mounted.
 *
 * What is asserted is the three actions 18 §7.3 names and the filter that makes the notice about the last run rather than
 * about every crash ever recorded. The window's five minutes is the same one `StartupVerdict` uses; if the two ever
 * disagreed, the banner would claim a crash that safe mode ignored, or stay silent about the one that caused it.
 */

import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  call: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call } }))

const CrashNotice = (await import('./CrashNotice.vue')).default
const { messages } = await import('../messages')

const RECENT = {
  atMs: Date.now(),
  version: '0.1.0',
  location: 'src/plugin.rs:42',
  message: 'the plugin panicked'
}

const OLD = {
  atMs: Date.now() - 6 * 60 * 1000,
  version: '0.1.0',
  location: 'src/elsewhere.rs:1',
  message: 'a crash from last week'
}

function report(value: unknown): { ok: true, value: unknown } {
  return { ok: true, value }
}

beforeEach(() => {
  mocks.call.mockReset()
})

describe('the crash notice', () => {
  it('shows a recent report and keeps the old ones out of the prompt', async () => {
    mocks.call.mockResolvedValueOnce(report([RECENT, OLD]))

    const wrapper = mount(CrashNotice)
    await flushPromises()

    expect(wrapper.text()).toContain(messages.crash.title)
    expect(wrapper.text()).toContain(messages.crash.detail(1))
    expect(wrapper.text()).not.toContain(OLD.message)
    wrapper.unmount()
  })

  it('stays silent when every report is older than the prompt window', async () => {
    mocks.call.mockResolvedValueOnce(report([OLD]))

    const wrapper = mount(CrashNotice)
    await flushPromises()

    expect(wrapper.text()).not.toContain(messages.crash.title)
    wrapper.unmount()
  })

  it('shows the details on request and deletes the reports on ignore', async () => {
    mocks.call.mockResolvedValueOnce(report([RECENT])).mockResolvedValueOnce(report(null))

    const wrapper = mount(CrashNotice)
    await flushPromises()

    const show = wrapper.findAll('button').find(button => button.text() === messages.crash.showDetails)
    expect(show, 'the details button is missing').toBeDefined()
    await show?.trigger('click')
    expect(wrapper.text()).toContain(RECENT.message)
    expect(wrapper.text()).toContain(RECENT.location)

    const ignore = wrapper.findAll('button').find(button => button.text() === messages.crash.ignore)
    expect(ignore, 'the ignore button is missing').toBeDefined()
    await ignore?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_crash_dismiss')
    expect(wrapper.text()).not.toContain(messages.crash.title)
    wrapper.unmount()
  })

  it('writes a bundle and names its path rather than only saying it worked', async () => {
    mocks.call
      .mockResolvedValueOnce(report([RECENT]))
      .mockResolvedValueOnce(report({ path: 'C:/data/synra-diagnostics.json', included: [], excluded: [] }))

    const wrapper = mount(CrashNotice)
    await flushPromises()

    const exportButton = wrapper.findAll('button').find(button => button.text() === messages.crash.exportBundle)
    expect(exportButton, 'the export button is missing').toBeDefined()
    await exportButton?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_export', {
      options: { includeAudit: false, includeDebugLogs: false }
    })
    expect(wrapper.text()).toContain('C:/data/synra-diagnostics.json')
    wrapper.unmount()
  })
})
