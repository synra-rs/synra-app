// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { field, messages, mountPalette, open, options, selected, snapshot, text, type, useDevicesStore, usePluginsStore } from './search-palette.testkit'

describe('the search box', () => {
  it('opens with command K and keeps composing Enter in the input', async () => {
    const { router } = mountPalette()
    await flushPromises()
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
    await flushPromises()
    expect(options().length).toBeGreaterThan(0)
    field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/')
    expect(document.querySelector('[role="dialog"]')).not.toBeNull()
  })

  it('finds a plugin by title and by id', async () => {
    usePluginsStore().applySnapshot([snapshot()])

    const { wrapper } = mountPalette()
    await open(wrapper)

    await type('start')
    expect(text()).toContain('Starter')

    await type('@acme')
    expect(text()).toContain('Starter')
  })

  it('groups what it found instead of returning one flat list', async () => {
    usePluginsStore().applySnapshot([snapshot()])

    const { wrapper } = mountPalette()
    await open(wrapper)

    await type('s')
    expect(text()).toContain(messages.search.groups.settings)
    expect(text()).toContain(messages.search.groups.pages)
  })

  it('says so when nothing matches', async () => {
    const { wrapper } = mountPalette()
    await open(wrapper)

    await type('zzzz-nothing')
    expect(text()).toContain(messages.search.empty)
  })

  it('takes a settings section to that section, not just to the page', async () => {
    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type(messages.settings.categories.diagnostics)
    const found = options()
    expect(found.length).toBeGreaterThan(0)
    found[0]!.click()
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/settings')
    expect(router.currentRoute.value.query.category).toBe('diagnostics')
  })

  it('takes a device to the devices page', async () => {
    useDevicesStore().applySnapshot({
      identity: { deviceId: 'self', displayName: 'This device', platform: 'Windows', kernelVersion: '0.1.0' },
      devices: [{ deviceId: 'peer-1', displayName: 'Studio laptop', platform: 'macOS', online: true, paired: true }]
    } as never)

    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type('studio')
    const found = options()
    expect(found.length).toBe(1)
    found[0]!.click()
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/devices')

    expect(router.currentRoute.value.query.device).toBe('peer-1')
  })

  it('moves through results with the arrow keys and takes one with Enter', async () => {
    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type('i')

    const before = selected()
    expect(before.length).toBe(1)
    const firstName = before[0]!.textContent

    field().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await flushPromises()
    const after = selected()
    expect(after.length).toBe(1)
    expect(after[0]!.textContent).not.toBe(firstName)

    field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.path).not.toBe('/')
  })

  it('paints hover and selection with different colours', async () => {
    const { wrapper } = mountPalette()
    await open(wrapper)
    await type('i')

    const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')]
    expect(options.length).toBeGreaterThan(1)
    const selectedRow = options.find(row => row.getAttribute('aria-selected') === 'true')
    expect(selectedRow, 'nothing is selected on arrival').toBeDefined()
    expect(selectedRow!.className).toContain('bg-raised')

    for (const row of options)
      expect(row.className).toContain('hover:bg-surface')
  })

  it('does not take a slash typed into a field', async () => {
    mountPalette()
    await flushPromises()

    const elsewhere = document.createElement('input')
    document.body.append(elsewhere)
    elsewhere.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).toBeNull()
    expect(text()).not.toContain(messages.search.scope)
  })

  it('stays shut while another modal is open', async () => {
    mountPalette()
    await flushPromises()

    const root = document.createElement('div')
    root.className = 'ant-modal-root'
    const wrap = document.createElement('div')
    wrap.className = 'ant-modal-wrap'
    root.append(wrap)
    document.body.append(root)

    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).toBeNull()
    expect(text()).not.toContain(messages.search.scope)
    root.remove()
  })

  it('opens on a slash pressed outside any field', async () => {
    mountPalette()
    await flushPromises()

    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).not.toBeNull()
    expect(text()).toContain(messages.search.scope)
  })

  it('shows what it searches only while the box is empty', async () => {
    const { wrapper } = mountPalette()
    await open(wrapper)

    expect(text()).toContain(messages.search.scope)

    await type('zzzz-nothing')
    expect(text()).not.toContain(messages.search.scope)
    expect(text()).toContain(messages.search.empty)
  })

  it('names the dialog it opens', async () => {
    const { wrapper } = mountPalette()
    try {
      await open(wrapper)
      const dialog = document.querySelector('[role="dialog"]')
      expect(dialog, 'the palette dialog is missing').not.toBeNull()
      expect(dialog?.getAttribute('aria-label')).toBe(messages.search.title)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('tells the reader which row the arrow keys moved to', async () => {
    const { wrapper } = mountPalette()
    try {
      await open(wrapper)
      await type('i')

      const first = field().getAttribute('aria-activedescendant')
      expect(first, 'the combobox names no active option').toBeTruthy()
      const firstRow = document.getElementById(first!)
      expect(firstRow, 'aria-activedescendant points at nothing').not.toBeNull()
      expect(firstRow!.getAttribute('aria-selected')).toBe('true')

      field().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
      await flushPromises()

      const second = field().getAttribute('aria-activedescendant')
      expect(second, 'the arrow key did not move the active option').not.toBe(first)
      const secondRow = document.getElementById(second!)
      expect(secondRow, 'aria-activedescendant points at nothing').not.toBeNull()
      expect(secondRow!.getAttribute('aria-selected')).toBe('true')
    }
    finally {
      wrapper.unmount()
    }
  })

  it('points at nothing once the last result is gone', async () => {
    const { wrapper } = mountPalette()
    try {
      await open(wrapper)
      await type('i')
      expect(field().getAttribute('aria-activedescendant'), 'nothing was selected to begin with').toBeTruthy()

      await type('zzzz-nothing')
      expect(options().length, 'the list did not empty, so this is not the state under test').toBe(0)
      expect(field().getAttribute('aria-activedescendant'), 'the combobox still names a row that is gone').toBeFalsy()
    }
    finally {
      wrapper.unmount()
    }
  })
})
