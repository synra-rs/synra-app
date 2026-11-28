// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { expect, it } from 'vitest'
import { registryFixture } from '../ipc/registry-fixture'
import { answer, ErrorNotice, IDENTITY, mocks, mountPage, openCategory } from './settings.testkit'

it('keeps a corrupt mirror visible instead of silently dropping it from the settings draft', async () => {
  answer({
    config_get: {
      values: {
        'plugins.registry': ['https://valid.example', 123]
      },
      overridden: ['plugins.registry']
    },
    identity_get: IDENTITY
  })
  const wrapper = await mountPage()
  try {
    await openCategory(wrapper, 'plugins')
    const inputs = wrapper.findAll('input[placeholder="https://registry.example.com"]')
    expect(inputs.map(input => (input.element as HTMLInputElement).value)).toEqual(['https://valid.example', '123'])
  }
  finally { wrapper.unmount() }
})

it('submits an empty mirror for validation rather than silently deleting it', async () => {
  answer({
    config_get: {
      values: {
        'plugins.registry': ['https://valid.example', 'https://other.example']
      },
      overridden: ['plugins.registry']
    },
    identity_get: IDENTITY
  })
  const read = mocks.call.getMockImplementation()!
  mocks.call.mockImplementation(async (command, args) => command === 'config_set'
    ? { ok: false, error: registryFixture.invalidConfiguration }
    : read(command, args))
  const wrapper = await mountPage()
  try {
    await openCategory(wrapper, 'plugins')
    await wrapper.findAll('input[placeholder="https://registry.example.com"]')[1]!.setValue('')
    await wrapper.findAll('button').find(button => button.text().includes('Save'))!.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: {
        set: {
          'plugins.registry': ['https://valid.example', '']
        }
      }
    })
    expect(wrapper.getComponent(ErrorNotice).props('code')).toBe('invalid_argument')
    expect(wrapper.text()).toContain('plugins.registry[1]')
  }
  finally { wrapper.unmount() }
})
