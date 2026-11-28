import type { DeviceCard } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { setLocalePreference } from '@/app/locale'
import { answer, DEVICES, HomePage, IDENTITY, INTENT, messages, mocks, router, useDevicesStore } from './index.testkit'

describe('the home page', () => {
  it.each(['en', 'zh-CN'])('shows activity data without built-in feature launchers in %s', async (language) => {
    setLocalePreference(language)
    answer({
      intent_list: [INTENT]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
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
      for (const retired of [
        'Share',
        'Share clipboard',
        '分享',
        '分享剪贴板'
      ])
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
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
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
        {
          fileName: 'notes.md',
          path,
          bytes: 2048,
          receivedAtMs
        }
      ]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
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
  async function drawWire(devices: readonly DeviceCard[]): Promise<{
    widths: string[]
    glows: boolean
  }> {
    useDevicesStore().applySnapshot({ identity: IDENTITY, devices })
    answer({})
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    try {
      await flushPromises()
      const bar = wrapper.get('.bg-primary')
      return {
        widths: bar.classes(),
        glows: bar.classes().includes('shadow-glow')
      }
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
    const wire = await drawWire([
      { ...DEVICES[0]!, online: false }
    ])
    expect(wire.widths).toContain('w-full')
    expect(wire.widths).toContain('opacity-40')
    expect(wire.glows).toBe(false)
  })
  it('draws nothing paired as a short stub', async () => {
    const wire = await drawWire([])
    expect(wire.widths).toContain('w-1/12')
    expect(wire.glows).toBe(false)
  })
})
