import { flushPromises } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { answer, IDENTITY, messages, mountPage, openDiagnostic, testRouter } from './settings.testkit'

describe('the settings page', () => {
  it('changes section when the query changes on the page already open', async () => {
    answer({ identity_get: IDENTITY, config_get: { values: {}, overridden: [] } })
    const wrapper = await mountPage('/settings')
    try {
      await flushPromises()
      const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'settings-category')!
      expect(select.props('value')).toBe('general')
      await testRouter().push('/settings?category=diagnostics')
      await flushPromises()
      expect(select.props('value')).toBe('diagnostics')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('names the log line count box', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      await openDiagnostic(wrapper, messages.settings.diagnostics.export)
      const debug = wrapper.findAllComponents({ name: 'ACheckbox' })
        .find(candidate => candidate.text() === messages.settings.diagnostics.debug)
      expect(debug, 'the debug-log checkbox is missing').toBeDefined()
      await debug?.vm.$emit('update:checked', true)
      await flushPromises()
      const markup = wrapper.get('[data-testid="settings-diagnostics"]').html()
      expect(markup).toContain(`aria-label="${messages.settings.diagnostics.lines}"`)
    }
    finally {
      wrapper.unmount()
    }
  })
})
