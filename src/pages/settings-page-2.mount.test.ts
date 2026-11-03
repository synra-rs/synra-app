import { flushPromises } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { answer, autostart, ErrorNotice, IDENTITY, messages, mocks, mountPage, NO_METRICS, openCategory, openDiagnostic } from './settings-page.testkit'

describe('the settings page', () => {
  it('writes one mirror as a string and several as an array', async () => {
    answer({
      config_get: {
        values: { 'plugins.registry': ['https://a.example', 'https://b.example'] },
        overridden: ['plugins.registry']
      },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'plugins')
    expect(wrapper.findAll('input[placeholder="https://registry.example.com"]')).toHaveLength(2)
    const remove = wrapper.findAll('button').find(button => button.text().includes('Remove'))
    expect(remove).toBeDefined()
    await remove?.trigger('click')
    await flushPromises()
    const save = wrapper.findAll('button').find(button => button.text().includes('Save'))
    await save?.trigger('click')
    await flushPromises()
    const saves = mocks.call.mock.calls.filter(call => call[0] === 'config_set')
    expect(saves.at(-1)).toEqual([
      'config_set',
      { patch: { set: { 'plugins.registry': 'https://b.example' } } }
    ])
    const add = wrapper.findAll('button').find(button => button.text().includes('Add mirror'))
    await add?.trigger('click')
    await flushPromises()
    const inputs = wrapper.findAll('input[placeholder="https://registry.example.com"]')
    expect(inputs).toHaveLength(2)
    const second = inputs[1]
    expect(second).toBeDefined()
    await second?.setValue('https://c.example')
    await save?.trigger('click')
    await flushPromises()
    const after = mocks.call.mock.calls.filter(call => call[0] === 'config_set')
    expect(after.at(-1)).toEqual([
      'config_set',
      { patch: { set: { 'plugins.registry': ['https://b.example', 'https://c.example'] } } }
    ])
    wrapper.unmount()
  })
  it('shows this device\'s name, which is the one field the page can state without being told', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    await flushPromises()
    expect(wrapper.text()).toContain(IDENTITY.displayName)
    wrapper.unmount()
  })
  it('offers both ways out of a name conflict, not just overwrite', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      const rename = wrapper.findAll('button').find(button => button.text() === messages.settings.field.rename)
      expect(rename, 'the rename button is missing').toBeDefined()
      await rename!.trigger('click')
      await wrapper.get('#device-name').setValue('Draft name')
      const { useDevicesStore } = await import('../stores/devices')
      useDevicesStore().applySnapshot({ identity: { ...IDENTITY, displayName: 'External name' }, devices: [] })
      await flushPromises()
      expect(wrapper.text()).toContain(messages.settings.edit.conflict)
      const load = wrapper.findAll('button').find(button => button.text() === messages.settings.edit.loadCurrent)
      expect(load, 'no way to take the other value').toBeDefined()
      await load!.trigger('click')
      expect((wrapper.get('#device-name').element as HTMLInputElement).value).toBe('External name')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('lists a trusted sender and takes the trust back by unsetting the last key', async () => {
    const sender = '11111111-1111-4111-8111-111111111111'
    answer({
      config_get: { values: { 'transfers.acceptFrom': [sender] }, overridden: ['transfers.acceptFrom'] },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'security')
    expect(wrapper.text()).toContain('Ask again')
    const remove = wrapper.findAll('button').find(button => button.text().includes('Ask again'))
    expect(remove, 'the trusted sender has a remove button').toBeDefined()
    await remove?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', { patch: { unset: ['transfers.acceptFrom'] } })
    wrapper.unmount()
  })
  it('shows the refusal when the configuration cannot be read', async () => {
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'config_get')
        return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
      if (command === 'identity_get')
        return { ok: true, value: IDENTITY }
      if (command === 'diag_metrics')
        return { ok: true, value: NO_METRICS }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })
    const wrapper = await mountPage()
    await flushPromises()
    const notice = wrapper.findComponent(ErrorNotice)
    expect(notice.exists()).toBe(true)
    expect(notice.props('code')).toBe('internal')
    wrapper.unmount()
  })
  it('draws the metrics the recorder holds, keyed the way the kernel names them', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: { name: 'host_calls_total', labels: [['plugin', '@acme/hello']] },
            value: { kind: 'counter', value: 7 }
          },
          {
            key: { name: 'host_call_ms', labels: [] },
            value: { kind: 'histogram', value: { bounds: [1, 10], buckets: [2, 3, 1], count: 6, sum: 30 } }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1000
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain('host_calls_total{plugin=@acme/hello}')
    expect(wrapper.text()).toContain('7')
    expect(wrapper.text()).toContain(messages.settings.metrics.histogram(6, '5.00'))
    wrapper.unmount()
  })
  it('polls metrics only while the performance section is visible', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    vi.useFakeTimers()
    const wrapper = await mountPage()
    try {
      await flushPromises()
      expect(mocks.call).not.toHaveBeenCalledWith('diag_metrics')
      await openDiagnostic(wrapper, messages.settings.metrics.title)
      const header = wrapper.findAll('[data-testid="settings-diagnostics"] .ant-collapse-header').find(value => value.text().includes(messages.settings.metrics.title))!
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(1)
      await vi.advanceTimersByTimeAsync(5000)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(2)
      await header.trigger('click')
      await vi.advanceTimersByTimeAsync(10000)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(2)
    }
    finally {
      wrapper.unmount()
      vi.useRealTimers()
    }
  })
  it('compares the whole start against the cold-start budget', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: { name: 'startup_duration_ms', labels: [['phase', 'state']] },
            value: { kind: 'histogram', value: { bounds: [1], buckets: [1, 0], count: 1, sum: 40 } }
          },
          {
            key: { name: 'startup_duration_ms', labels: [['phase', 'total']] },
            value: { kind: 'histogram', value: { bounds: [1, 10, 100, 1000], buckets: [0, 0, 0, 2, 0], count: 2, sum: 700 } }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1000
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain(messages.settings.metrics.startup)
    expect(wrapper.text()).toContain('350 ms')
    expect(wrapper.text()).toContain(messages.settings.metrics.startupWithin)
    wrapper.unmount()
  })
  it('asks the platform to start at login', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    await flushPromises()
    const launch = wrapper.findAllComponents({ name: 'ASwitch' }).find(value => value.attributes('aria-label') === messages.settings.startup.launchAtLogin)
    expect(launch).toBeDefined()
    await launch?.vm.$emit('change', true)
    await flushPromises()
    expect(autostart.enable).toHaveBeenCalled()
    wrapper.unmount()
  })
  it('says when the snapshot is incomplete rather than drawing it as the whole', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [{ key: { name: 'host_calls_total', labels: [] }, value: { kind: 'counter', value: 1 } }],
        overflowedTotal: 3,
        maxSeriesPerMetric: 2
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain(messages.settings.metrics.overflow(3, 2))
    wrapper.unmount()
  })
})
