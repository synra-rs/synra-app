import type { PluginSnapshot } from '@synra/protocol'
import { expect, it, vi } from 'vitest'
import { KERNEL_EVENT_CHANNEL } from './channel'
import { FakeKernel } from './fake-kernel'

it('omits empty denied collections in reads and consent responses like the real kernel', async () => {
  const bridge = new FakeKernel()
  const plugins = await bridge.invoke('plugins_list') as readonly PluginSnapshot[]
  const plugin = plugins.find(plugin => plugin.pluginId === '@synra-plugin/github')!
  expect(plugin).not.toHaveProperty('denied')
  const onEvent = vi.fn()
  await bridge.listen(KERNEL_EVENT_CHANNEL, onEvent)
  const response = await bridge.invoke('plugins_consent_respond', { pluginId: plugin.pluginId, granted: plugin.requested })
  expect(response).not.toHaveProperty('denied')
  expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({ type: 'pluginUpdated', payload: response }))
})

it('requires explicit approval for a zero-permission plugin and retains it across disabling', async () => {
  const bridge = new FakeKernel()
  const args = { pluginId: '@preview/notes' }
  await bridge.invoke('plugins_enable', args)
  const before = await bridge.invoke('plugins_list') as readonly PluginSnapshot[]
  expect(before.find(plugin => plugin.pluginId === args.pluginId)?.state).toBe('awaiting_consent')
  expect(await bridge.invoke('plugins_consent_respond', { ...args, granted: ['log:*'] })).toMatchObject({ code: 'invalid_argument' })
  expect(await bridge.invoke('plugins_consent_respond', { ...args, granted: [] })).toMatchObject({ state: 'active', granted: [] })
  await bridge.invoke('plugins_disable', args)
  await bridge.invoke('plugins_enable', args)
  const after = await bridge.invoke('plugins_list') as readonly PluginSnapshot[]
  expect(after.find(plugin => plugin.pluginId === args.pluginId)?.state).toBe('active')
})
