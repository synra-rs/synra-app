import { flushPromises } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { answer, ErrorNotice, IDENTITY, messages, mocks, mountPage, NO_METRICS, openCategory, openDiagnostic, testRouter } from './settings-page.testkit'

describe('the settings page', () => {
  it('keeps a rejected draft in its own section and retries the same edit explicitly', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const fallback = mocks.call.getMockImplementation()!
    let refused = true
    mocks.call.mockImplementation((command: string, args?: unknown) => command === 'config_set' && refused
      ? { ok: false, error: { code: 'invalidArgument', message: 'error.invalidArgument', retryable: false } }
      : fallback(command, args))
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openCategory(wrapper, 'plugins')
      await wrapper.get('#registry-0').setValue('https://draft.example')
      await wrapper.get('[data-testid="save-registry"]').trigger('click')
      await flushPromises()
      expect((wrapper.get('#registry-0').element as HTMLInputElement).value).toBe('https://draft.example')
      expect(wrapper.get('[data-testid="settings-plugins"]').findComponent(ErrorNotice).exists()).toBe(true)
      expect(wrapper.get('[data-testid="settings-general"]').findComponent(ErrorNotice).exists()).toBe(false)
      refused = false
      await wrapper.get('[data-testid="save-registry"]').trigger('click')
      await flushPromises()
      expect(wrapper.get('[data-testid="settings-plugins"]').findComponent(ErrorNotice).exists()).toBe(false)
      expect(wrapper.get('[data-testid="save-registry"]').attributes('disabled')).toBeDefined()
    }
    finally {
      wrapper.unmount()
    }
  })
  it('checks a submitted rename without writing it twice after a failed status read', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, identity_set_display_name: null })
    const fallback = mocks.call.getMockImplementation()!
    let reads = 0
    mocks.call.mockImplementation((command: string, args?: unknown) => {
      if (command === 'identity_get') {
        reads++
        if (reads === 2)
          return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: true } }
        return { ok: true, value: reads > 2 ? { ...IDENTITY, displayName: 'Desk' } : IDENTITY }
      }
      return fallback(command, args)
    })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.settings.field.rename)!.trigger('click')
      await wrapper.get('#device-name').setValue('Desk')
      await wrapper.findAll('button').find(button => button.text() === messages.settings.field.save)!.trigger('click')
      await flushPromises()
      expect((wrapper.get('#device-name').element as HTMLInputElement).value).toBe('Desk')
      await wrapper.findAll('button').find(button => button.text() === messages.settings.edit.check)!.trigger('click')
      await flushPromises()
      expect(mocks.call.mock.calls.filter(([command]) => command === 'identity_set_display_name')).toHaveLength(1)
      expect(wrapper.find('#device-name').exists()).toBe(false)
      const { useDevicesStore } = await import('../stores/devices')
      expect(useDevicesStore().identity?.displayName).toBe('Desk')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('keeps editing when navigation is cancelled and only discards after an explicit decision', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openCategory(wrapper, 'plugins')
      await wrapper.get('#registry-0').setValue('https://draft.example')
      const cancelled = testRouter().push('/')
      await flushPromises()
      const modal = wrapper.findAllComponents({ name: 'AModal' }).find(value => value.props('open'))!
      expect(modal.exists()).toBe(true)
      await modal.vm.$emit('cancel')
      await cancelled
      expect(testRouter().currentRoute.value.path).toBe('/settings')
      expect((wrapper.get('#registry-0').element as HTMLInputElement).value).toBe('https://draft.example')
      const leaving = testRouter().push('/')
      await flushPromises()
      const button = [...document.querySelectorAll<HTMLButtonElement>('.ant-modal-footer button')].find(value => value.textContent === messages.settings.edit.discard)!
      button.click()
      await leaving
      expect(testRouter().currentRoute.value.path).toBe('/')
      expect(mocks.call.mock.calls.some(([command]) => command === 'config_set')).toBe(false)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('keeps the draft and route when saving before navigation fails', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openCategory(wrapper, 'plugins')
      await wrapper.get('#registry-0').setValue('https://draft.example')
      const leaving = testRouter().push('/')
      await flushPromises()
      const button = [...document.querySelectorAll<HTMLButtonElement>('.ant-modal-footer button')].find(value => value.textContent === messages.settings.edit.saveAll)!
      button.click()
      await leaving
      await flushPromises()
      expect(testRouter().currentRoute.value.path).toBe('/settings')
      expect((wrapper.get('#registry-0').element as HTMLInputElement).value).toBe('https://draft.example')
      expect(wrapper.get('[data-testid="settings-plugins"]').findComponent(ErrorNotice).exists()).toBe(true)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('does not submit a registry draft while an input method is composing', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openCategory(wrapper, 'plugins')
      await wrapper.get('#registry-0').setValue('https://draft.example')
      await wrapper.get('#registry-0').trigger('keydown', { key: 'Enter', keyCode: 13, isComposing: true })
      await flushPromises()
      expect(mocks.call.mock.calls.some(([command]) => command === 'config_set')).toBe(false)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('pauses metrics on hidden windows and category changes then resumes once', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    vi.useFakeTimers()
    const wrapper = await mountPage()
    const count = () => mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics').length
    try {
      await flushPromises()
      await openDiagnostic(wrapper, messages.settings.metrics.title)
      expect(count()).toBe(1)
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' })
      document.dispatchEvent(new Event('visibilitychange'))
      await vi.advanceTimersByTimeAsync(15000)
      expect(count()).toBe(1)
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
      document.dispatchEvent(new Event('visibilitychange'))
      await flushPromises()
      expect(count()).toBe(2)
      await openCategory(wrapper, 'general')
      await vi.advanceTimersByTimeAsync(15000)
      expect(count()).toBe(2)
      await openCategory(wrapper, 'diagnostics')
      expect(count()).toBe(3)
    }
    finally {
      wrapper.unmount()
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
      vi.useRealTimers()
    }
  })
  it('discards old metric responses and never overlaps pending requests when reopened', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const fallback = mocks.call.getMockImplementation()!
    let finish: ((value: unknown) => void) | undefined
    let requests = 0
    mocks.call.mockImplementation((command: string, args?: unknown) => {
      if (command !== 'diag_metrics')
        return fallback(command, args)
      requests++
      if (requests === 1)
        return new Promise((resolve) => { finish = resolve })
      return { ok: true, value: NO_METRICS }
    })
    vi.useFakeTimers()
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openDiagnostic(wrapper, messages.settings.metrics.title)
      await openCategory(wrapper, 'general')
      await openCategory(wrapper, 'diagnostics')
      await vi.advanceTimersByTimeAsync(20000)
      expect(requests).toBe(1)
      finish!({ ok: true, value: { ...NO_METRICS, values: [{ key: { name: 'old_series', labels: [] }, value: { kind: 'counter', value: 10 } }] } })
      await flushPromises()
      expect(requests).toBe(2)
      expect(wrapper.text()).not.toContain('old_series')
      expect(wrapper.text()).toContain(messages.settings.metrics.empty)
    }
    finally {
      wrapper.unmount()
      vi.useRealTimers()
    }
  })
  it('keeps diagnostic errors independent when another tool succeeds', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, diag_logs_tail: ['recent log'] })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openDiagnostic(wrapper, messages.settings.diagnostics.probesTitle)
      await wrapper.findAll('button').find(value => value.text() === messages.settings.diagnostics.probes)!.trigger('click')
      await flushPromises()
      await openDiagnostic(wrapper, messages.settings.diagnostics.logs)
      expect(wrapper.text()).toContain('recent log')
      expect(wrapper.findComponent(ErrorNotice).exists()).toBe(true)
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('internal')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('opens the section a query asked for', async () => {
    answer({ identity_get: IDENTITY, config_get: { values: {}, overridden: [] } })
    const wrapper = await mountPage('/settings?category=diagnostics')
    try {
      await flushPromises()
      const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'settings-category')!
      expect(select.props('value')).toBe('diagnostics')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('ignores a query that names no section', async () => {
    answer({ identity_get: IDENTITY, config_get: { values: {}, overridden: [] } })
    const wrapper = await mountPage('/settings?category=not-a-section')
    try {
      await flushPromises()
      const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'settings-category')!
      expect(select.props('value')).toBe('general')
    }
    finally {
      wrapper.unmount()
    }
  })
})
