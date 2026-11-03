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
