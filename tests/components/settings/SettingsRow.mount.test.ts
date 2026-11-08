// @vitest-environment jsdom
/**
 * `SettingsRow` — the label/field pairing every settings field is built on.
 *
 * # What is asserted
 *
 * Two things that are invisible when they break:
 *
 * 1. **The label becomes a `<label for>` only when there is a field to point at.** A row with no `forId` must not
 *    render a `<label>` with a dangling `for` — clicking it would do nothing and a screen reader would announce a
 *    control that is not there.
 * 2. **Feedback gets its own row.** Errors and save results are slotted into `feedback`; dropping that slot (or
 *    forgetting `sm:col-span-2`) puts the message in the narrow control column, under the button.
 *
 * The grid classes are asserted as strings because jsdom has no layout engine.
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import SettingsRow from '@/components/settings/SettingsRow.vue'

describe('settings row', () => {
  it('points its label at the field when it has an id', () => {
    const wrapper = mount(SettingsRow, { props: { label: 'Device name', forId: 'device-name' } })
    const label = wrapper.get('label')
    expect(label.attributes('for')).toBe('device-name')
    expect(label.text()).toBe('Device name')
    wrapper.unmount()
  })

  it('renders plain text when there is no field to point at', () => {
    // A `<label for="undefined">` is worse than no label: the browser scrolls nowhere and the association is false.
    const wrapper = mount(SettingsRow, { props: { label: 'About' } })
    expect(wrapper.find('label').exists()).toBe(false)
    expect(wrapper.text()).toContain('About')
    wrapper.unmount()
  })

  it('shows the description under the label when one is given', () => {
    const wrapper = mount(SettingsRow, { props: { label: 'Policy', description: 'When updates may install.' } })
    expect(wrapper.text()).toContain('When updates may install.')
    wrapper.unmount()
  })

  it('leaves the description out when there is none', () => {
    // Otherwise an empty paragraph adds a gap under every bare row.
    const wrapper = mount(SettingsRow, { props: { label: 'Policy' } })
    expect(wrapper.findAll('p')).toHaveLength(0)
    wrapper.unmount()
  })

  it('gives feedback a full-width row of its own', () => {
    const wrapper = mount(SettingsRow, {
      props: { label: 'Policy' },
      slots: { feedback: '<p class="feedback">Saved.</p>' }
    })
    const message = wrapper.get('.feedback')
    const row = message.element.parentElement
    expect(row?.className).toContain('sm:col-span-2')
    wrapper.unmount()
  })

  it('leaves the feedback row out when nothing is slotted', () => {
    const wrapper = mount(SettingsRow, { props: { label: 'Policy' } })
    // Two columns only: the label column and the control column.
    expect(wrapper.findAll('.sm\\:col-span-2')).toHaveLength(0)
    wrapper.unmount()
  })
})
