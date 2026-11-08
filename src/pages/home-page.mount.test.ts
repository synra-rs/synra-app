import type { DeviceCard } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { setLocalePreference } from '../app/locale'
import { formatTime } from '../utils/locale-format'
import { answer, DEVICES, HomePage, IDENTITY, INTENT, messages, mocks, router, useDevicesStore } from './home-page.testkit'

describe('the home page', () => {
  it.each(['en', 'zh-CN'])('shows activity data without built-in feature launchers in %s', async (language) => {
    setLocalePreference(language)
    answer({ intent_list: [INTENT] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    try {
      await flushPromises()
      expect(wrapper.text()).toContain(messages.home.received.title)
      expect(wrapper.text()).toContain(messages.home.received.states.succeeded)
      expect(wrapper.text()).toContain('@synra-plugin/github')
      expect(wrapper.text()).not.toContain('01HQ00000000000000000TRACE1')
      const details = wrapper.findAll('button').find(button => button.text() === messages.home.received.details)
      expect(details).toBeDefined()
      await details!.trigger('click')
      expect(wrapper.text()).toContain('01HQ00000000000000000TRACE1')
      const labels = wrapper.findAll('button').map(button => button.text())
      for (const retired of ['Share', 'Share clipboard', '分享', '分享剪贴板'])
        expect(labels).not.toContain(retired)
      expect(wrapper.findComponent({ name: 'AInput' }).exists()).toBe(false)
      expect(mocks.call.mock.calls.every(([command]) => ['intent_list', 'transfer_list', 'transfer_received'].includes(command))).toBe(true)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('asks for the intents when it is mounted', async () => {
    answer({ intent_list: [] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('intent_list')
    wrapper.unmount()
  })
  it('shows the four facts a received file row owes', async () => {
    const receivedAtMs = Date.UTC(2026, 9, 30, 4, 5, 6)
    const path = 'C:\\Users\\someone\\AppData\\Roaming\\Synra\\inbox\\a-rather-long-folder-name\\notes.md'
    answer({
      intent_list: [],
      transfer_received: [
        { fileName: 'notes.md', path, bytes: 2048, receivedAtMs }
      ]
    })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    try {
      const text = wrapper.text()
      expect(text).toContain('notes.md')
      expect(text).toContain(path)
      expect(text).toContain('2.0 KiB')
      expect(text).toContain(messages.home.receivedFiles.receivedAt)
      expect(text).not.toContain(String(receivedAtMs))
      const shown = wrapper.findAll('span').find(span => span.text() === path)
      expect(shown, 'the full path is not rendered').toBeDefined()
      expect(shown!.classes()).not.toContain('truncate')
      expect(shown!.attributes('title')).toBe(path)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('lets the user accept an intent on a candidate device', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT1',
      traceId: '01HQ00000000000000000TRACE1',
      state: 'choosing',
      candidates: [
        { pluginId: '@synra/notes', title: 'Notes', deviceId: '11111111-1111-4111-8111-111111111111', online: true }
      ],
      startedAtMs: 1
    }
    answer({ intent_list: [intent], intent_decide: null })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    const accept = wrapper.findAll('button').find(button => button.text() === messages.home.received.accept)
    expect(accept, 'the accept action is missing').toBeDefined()
    const idShown = wrapper.findAll('span').find(span => span.attributes('title') === intent.candidates[0]!.deviceId)
    expect(idShown, 'the candidate id has no title').toBeDefined()
    await accept?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'accept', deviceId: intent.candidates[0]!.deviceId }
    })
    wrapper.unmount()
  })
  it('lets the user decline an intent while it is still matching', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT2',
      traceId: '01HQ00000000000000000TRACE2',
      state: 'matching',
      candidates: [],
      startedAtMs: 1
    }
    answer({ intent_list: [intent], intent_decide: null })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(wrapper.text()).toContain(messages.home.received.states.matching)
    const decline = wrapper.findAll('button').find(button => button.text() === messages.home.received.decline)
    expect(decline, 'a share still matching cannot be refused').toBeDefined()
    await decline?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'decline', reason: null }
    })
    wrapper.unmount()
  })
  it.each([
    ['matching', 'processing'],
    ['choosing', 'processing'],
    ['confirming', 'processing'],
    ['dispatching', 'processing'],
    ['succeeded', 'success'],
    ['noHandler', 'warning'],
    ['failed', 'error'],
    ['timedOut', 'error'],
    ['refused', 'error'],
    ['cancelled', 'default']
  ])('colours a %s intent %s', async (state, colour) => {
    answer({ intent_list: [{ intentId: 'i1', traceId: 't1', state, candidates: [], startedAtMs: 1 }] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    try {
      await flushPromises()
      const label = (messages.home.received.states as Record<string, string>)[state]
      const tag = wrapper.findAllComponents({ name: 'ATag' }).find(candidate => candidate.text() === label)
      expect(tag, `no tag for ${state}`).toBeDefined()
      expect(tag?.props('color')).toBe(colour)
    }
    finally {
      wrapper.unmount()
    }
  })
  async function drawWire(devices: readonly DeviceCard[]): Promise<{
    widths: string[]
    glows: boolean
  }> {
    useDevicesStore().applySnapshot({ identity: IDENTITY, devices })
    answer({})
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    try {
      await flushPromises()
      const bar = wrapper.get('.bg-primary')
      return { widths: bar.classes(), glows: bar.classes().includes('shadow-glow') }
    }
    finally {
      wrapper.unmount()
    }
  }
  it('draws a connected wire as the full, lit line', async () => {
    const wire = await drawWire([DEVICES[0]!])
    expect(wire.widths).toContain('w-full')
    expect(wire.glows).toBe(true)
  })
  it('draws an offline wire dimmer than a connected one', async () => {
    const wire = await drawWire([{ ...DEVICES[0]!, online: false }])
    expect(wire.widths).toContain('w-full')
    expect(wire.widths).toContain('opacity-40')
    expect(wire.glows).toBe(false)
  })
  it('draws nothing paired as a short stub', async () => {
    const wire = await drawWire([])
    expect(wire.widths).toContain('w-1/12')
    expect(wire.glows).toBe(false)
  })
  it('gives the details toggle a full-size target', async () => {
    answer({
      intent_list: [{ intentId: 'i1', traceId: 't1', state: 'succeeded', candidates: [], startedAtMs: 1 }]
    })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    try {
      await flushPromises()
      const toggle = wrapper.findAll('button').find(button => button.attributes('aria-expanded') !== undefined)
      expect(toggle, 'the details toggle is missing').toBeDefined()
      expect(toggle!.classes()).toContain('min-h-11')
      expect(wrapper.html()).toContain('md:min-h-8')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('says how many times a dispatch has been retried, and when it gave up', async () => {
    const nextAtMs = Date.UTC(2026, 9, 22, 10, 42)
    const retrying = {
      intentId: '01HQ00000000000000000INTNT4',
      traceId: '01HQ00000000000000000TRACE4',
      state: 'dispatching',
      candidates: [],
      retry: { attempts: 3, maxAttempts: 8, gaveUp: false, nextAtMs },
      startedAtMs: 1
    }
    const gaveUp = {
      intentId: '01HQ00000000000000000INTNT5',
      traceId: '01HQ00000000000000000TRACE5',
      state: 'dispatching',
      candidates: [],
      retry: { attempts: 8, maxAttempts: 8, gaveUp: true },
      startedAtMs: 1
    }
    answer({ intent_list: [retrying, gaveUp] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    const clock = formatTime(nextAtMs, true)
    expect(wrapper.text()).toContain(`Retrying (3/8), next try at ${clock}`)
    expect(wrapper.text()).toContain(messages.home.received.noAnswer)
    wrapper.unmount()
  })
  it('says why an intent failed without printing the kernel key', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT3',
      traceId: '01HQ00000000000000000TRACE3',
      state: 'failed',
      candidates: [],
      error: { code: 'plugin_not_installed', message: 'error.somethingUnmapped', retryable: false },
      startedAtMs: 1
    }
    answer({ intent_list: [intent] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(wrapper.text()).toContain(messages.errors.plugin_not_installed)
    expect(wrapper.text()).not.toContain('error.somethingUnmapped')
    wrapper.unmount()
  })
})
