// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { Segmented } from 'ant-design-vue'
import { expect, it } from 'vitest'
import { answer, click, ErrorNotice, messages, mocks, mountPage, RegistryDiscover, RegistryInstallDialog } from './workflow.testkit'

const item = { pluginId: '@acme/article-reader', title: 'Article reader', publisher: '@acme', version: '1.0', capabilityCount: 1, installed: false }
const preview = { ...item, hasUi: true, headless: false, capabilities: ['fs:read'] }
async function discover() {
  const page = await mountPage()
  await page.wrapper.getComponent(Segmented).vm.$emit('update:value', 'discover')
  await flushPromises()
  return { ...page, discover: page.wrapper.getComponent(RegistryDiscover) }
}
it('submits registry queries explicitly and opens the result without another id field', async () => {
  answer({ plugins_catalog_search: [item], plugins_preview: preview })
  const { discover: view } = await discover()
  expect(mocks.call.mock.calls.some(([name]) => name === 'plugins_catalog_search')).toBe(false)
  await view.get('input').setValue('article')
  await click(view, messages.plugins.catalog.action)
  expect(mocks.call).toHaveBeenCalledWith('plugins_catalog_search', { query: { query: 'article', offset: 0, sort: 'relevance' } })
  await click(view, messages.plugins.workflow.details)
  expect(mocks.call).toHaveBeenCalledWith('plugins_preview', { pluginId: item.pluginId })
  expect(document.querySelector('.ant-modal')?.textContent).toContain(item.title)
})
it('keeps a preview after failed installation and sends only one pending install', async () => {
  let finish!: (value: unknown) => void
  answer({ plugins_catalog_search: [item], plugins_preview: preview, plugins_install: () => new Promise(resolve => finish = resolve) })
  const { discover: view } = await discover()
  await view.get('input').setValue(item.pluginId)
  await click(view, messages.plugins.catalog.action)
  await click(view, messages.plugins.workflow.details)
  const dialog = view.getComponent(RegistryInstallDialog)
  await click(dialog, messages.plugins.installAction)
  await click(dialog, messages.plugins.installAction)
  expect(mocks.call.mock.calls.filter(([name]) => name === 'plugins_install')).toHaveLength(1)
  finish({ ok: false, error: { code: 'unavailable', message: 'error.unavailable' } })
  await flushPromises()
  expect(document.querySelector('.ant-modal')?.textContent).toContain(item.title)
  expect(dialog.getComponent(ErrorNotice).props('code')).toBe('unavailable')
})
it('offers registry settings for an unconfigured source', async () => {
  answer({ plugins_catalog_search: () => ({ ok: false, error: { code: 'unsupported', message: 'no registry is configured' } }) })
  const { discover: view, router } = await discover()
  await view.get('input').setValue('reader')
  await click(view, messages.plugins.catalog.action)
  await click(view, messages.actions.openSettings)
  expect(router.currentRoute.value.query.category).toBe('plugins')
})
it('keeps composing Enter in the search field', async () => {
  const { discover: view } = await discover()
  await view.get('input').setValue('reader')
  await view.get('input').trigger('keydown', { key: 'Enter', isComposing: true })
  expect(mocks.call.mock.calls.some(([name]) => name === 'plugins_catalog_search')).toBe(false)
})
