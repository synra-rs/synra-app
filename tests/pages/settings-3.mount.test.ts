import { flushPromises } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { answer, IDENTITY, messages, mocks, mountPage, openCategory, openDiagnostic, testRouter } from './settings.testkit'

describe('the settings page', () => {
  it('shows the audit retention and writes a new window', async () => {
    answer({
      config_get: { values: { 'audit.retentionDays': 30 }, overridden: ['audit.retentionDays'] },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'plugins')
    const number = wrapper.findComponent({ name: 'AInputNumber' })
    expect(number.exists(), 'the retention control is missing').toBe(true)
    expect(number.props('value'), 'the configured window reaches the box').toBe(30)
    await number.vm.$emit('update:value', 90)
    await flushPromises()
    const button = number.element.parentElement?.querySelector('button')
    expect(button, 'the box drew no way to save it').toBeDefined()
    button?.click()
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'audit.retentionDays': 90 } }
    })
    wrapper.unmount()
  })
  it('exports a diagnostic bundle and shows what it holds', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_export: {
        path: 'C:/data/synra-diagnostics.json',
        included: ['environment', 'config'],
        excluded: ['audit trail: not requested']
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.diagnostics.export)
    const boxes = wrapper.findAllComponents({ name: 'ACheckbox' })
    expect(boxes.length, 'the two optional sections are not drawn').toBeGreaterThanOrEqual(2)
    await boxes[0]?.vm.$emit('update:checked', true)
    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.settings.diagnostics.export)
    expect(button, 'the export button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('diag_export', {
      options: { includeAudit: true, includeDebugLogs: false }
    })
    expect(wrapper.text()).toContain('C:/data/synra-diagnostics.json')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.included('environment, config'))
    expect(wrapper.text()).toContain(messages.settings.diagnostics.excluded('audit trail: not requested'))
    wrapper.unmount()
  })
  it('dismisses the crash reports that would start the next run in safe mode', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_crash_dismiss: null
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.diagnostics.crashesTitle)
    const button = wrapper
      .findAll('button')
      .find(candidate => candidate.text() === messages.settings.diagnostics.dismissCrashes)
    expect(button, 'the dismiss button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()
    expect(mocks.call).not.toHaveBeenCalledWith('diag_crash_dismiss')
    const confirmation = wrapper.findAllComponents({ name: 'AModal' }).find(value => value.props('open'))!
    await confirmation.vm.$emit('ok')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('diag_crash_dismiss')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.dismissed)
    wrapper.unmount()
  })
  it('shows the crash reports on request, newest first as the kernel answers them', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_crashes: [
        {
          atMs: 2000,
          version: '0.1.0',
          location: 'src/plugin.rs:42',
          message: 'the plugin panicked'
        }
      ]
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.diagnostics.crashesTitle)
    const button = wrapper
      .findAll('button')
      .find(candidate => candidate.text() === messages.settings.diagnostics.showCrashes)
    expect(button, 'the show button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('diag_crashes')
    expect(wrapper.text()).toContain('the plugin panicked')
    expect(wrapper.text()).toContain('src/plugin.rs:42')
    wrapper.unmount()
  })
  it('reads the recent log when its section is opened', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_logs_tail: ['synra: started', 'synra: discovery bound']
    })
    const wrapper = await mountPage()
    await flushPromises()
    expect(mocks.call).not.toHaveBeenCalledWith('diag_logs_tail', expect.anything())
    await openDiagnostic(wrapper, messages.settings.diagnostics.logs)
    expect(mocks.call).toHaveBeenCalledWith('diag_logs_tail', { lines: 200 })
    expect(wrapper.text()).toContain('synra: discovery bound')
    wrapper.unmount()
  })
  it('runs the startup probes on request and shows each result', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_probes: {
        results: [
          { name: 'tcp_port_available', status: 'ok', detail: 'a free port was found' },
          { name: 'wasm_backend', status: 'warn', detail: 'this host did not read the runtime', suggestion: 'include this report' }
        ]
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.diagnostics.probesTitle)
    expect(mocks.call).not.toHaveBeenCalledWith('diag_probes')
    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.settings.diagnostics.probes)
    expect(button, 'the probe button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('diag_probes')
    expect(wrapper.text()).not.toContain('tcp_port_available')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.probeStatus('warn'))
    expect(wrapper.text()).toContain('include this report')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.probesSummary(1, 1, 0))
    const filter = wrapper.findAllComponents({ name: 'ACheckbox' }).at(-1)
    expect(filter, 'the only-problems filter is missing').toBeDefined()
    await filter?.vm.$emit('update:checked', false)
    await flushPromises()
    expect(wrapper.text()).toContain('tcp_port_available')
    expect(wrapper.text()).toContain('wasm_backend')
    wrapper.unmount()
  })
  it('shows a probe answer it does not recognise without calling it a failure', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_probes: { results: [{ name: 'skipped_by_policy', status: 'skipped', detail: 'policy' }] }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.diagnostics.probesTitle)
    await wrapper.findAll('button').find(candidate => candidate.text() === messages.settings.diagnostics.probes)?.trigger('click')
    await flushPromises()
    const text = wrapper.text()
    expect(text).toContain(messages.settings.diagnostics.probeStatus('skipped'))
    expect(text).not.toContain(messages.settings.diagnostics.probeStatus('fail'))
    const tag = wrapper.findAllComponents({ name: 'ATag' }).find(candidate => candidate.text() === messages.settings.diagnostics.probeStatus('skipped'))
    expect(tag, 'the unknown-status tag is missing').toBeDefined()
    expect(tag?.props('color')).toBe('default')
    wrapper.unmount()
  })
  it('drops the section from the url once a column is chosen by hand', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage('/settings?category=security')
    try {
      await flushPromises()
      expect(testRouter().currentRoute.value.query.category).toBe('security')
      await openCategory(wrapper, 'general')
      expect(testRouter().currentRoute.value.query.category).toBeUndefined()
    }
    finally {
      wrapper.unmount()
    }
  })
  it('keeps drafts across categories and exposes external changes without replacing them', async () => {
    answer({ config_get: { values: { 'plugins.registry': 'https://a.example' }, overridden: ['plugins.registry'] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openCategory(wrapper, 'plugins')
      await wrapper.get('#registry-0').setValue('https://draft.example')
      await openCategory(wrapper, 'general')
      expect(wrapper.get('[data-testid="settings-plugins"]').isVisible()).toBe(false)
      const { useSettingsStore } = await import('@/stores/settings')
      useSettingsStore().applySnapshot({ values: { 'plugins.registry': 'https://external.example' }, overridden: ['plugins.registry'] })
      await openCategory(wrapper, 'plugins')
      expect((wrapper.get('#registry-0').element as HTMLInputElement).value).toBe('https://draft.example')
      expect(wrapper.text()).toContain(messages.settings.edit.conflict)
      expect(wrapper.get('[data-testid="save-registry"]').text()).toBe(messages.settings.edit.overwrite)
      const current = wrapper.findAll('button').find(button => button.text() === messages.settings.edit.loadCurrent)!
      await current.trigger('click')
      expect((wrapper.get('#registry-0').element as HTMLInputElement).value).toBe('https://external.example')
    }
    finally {
      wrapper.unmount()
    }
  })
})
