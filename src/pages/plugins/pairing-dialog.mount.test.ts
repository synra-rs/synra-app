// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { Modal } from 'ant-design-vue'
import { describe, expect, it } from 'vitest'
import { answer, click, LocalInstallDialog, messages, mocks, mountPage, PairingPrompt, usePairingStore } from './workflow.testkit'

const peer = { code: '813642', deviceId: 'peer', displayName: 'Android', platform: 'android' }
describe('pairing presentation', () => {
  it('keeps the initiator waiting with a code and only a cancel action', async () => {
    const pairing = usePairingStore()
    pairing.apply({ type: 'pairingUpdated', payload: { phase: 'waiting', offer: peer } })
    const wrapper = mount(PairingPrompt, { attachTo: document.body })
    try {
      await flushPromises()
      expect(wrapper.get('output').text()).toBe(peer.code)
      expect(wrapper.text()).toContain(messages.devices.pairingWaiting(peer.displayName))
      expect(wrapper.text()).not.toContain(messages.devices.pairingAccept)
      expect(pairing.snapshot.phase).toBe('waiting')
    }
    finally { wrapper.unmount() }
  })
  it.each([[true, false], [false, false], [false, true]])('waits for the receiver decision accepted=%s block=%s', async (accepted, block) => {
    answer({ devices_pairing_respond: () => ({ ok: true, value: { phase: 'done', outcome: accepted ? 'paired' : 'refused' } }) })
    const pairing = usePairingStore()
    pairing.apply({ type: 'pairingUpdated', payload: { phase: 'asked', ask: peer } })
    const wrapper = mount(PairingPrompt, { attachTo: document.body })
    try {
      await flushPromises()
      expect(wrapper.get('output').text()).toBe(peer.code)
      expect(mocks.call).not.toHaveBeenCalled()
      await click(wrapper, accepted ? messages.devices.pairingAccept : block ? messages.devices.pairingBlock : messages.devices.pairingRefuse)
      expect(mocks.call).toHaveBeenCalledWith('devices_pairing_respond', { accepted, block })
      expect(pairing.snapshot.outcome).toBe(accepted ? 'paired' : 'refused')
    }
    finally { wrapper.unmount() }
  })
  it('restores a local install draft after a received pairing request is handled', async () => {
    answer({ devices_pairing_respond: () => ({ ok: true, value: { phase: 'done', outcome: 'refused' } }) })
    const { wrapper: page } = await mountPage()
    await click(page, messages.plugins.workflow.localInstall)
    const editor = page.getComponent(LocalInstallDialog)
    await editor.get('input').setValue('D:/chosen/source.synra')
    const prompt = mount(PairingPrompt, { attachTo: document.body })
    try {
      const pairing = usePairingStore()
      pairing.apply({ type: 'pairingUpdated', payload: { phase: 'asked', ask: peer } })
      await flushPromises()
      expect(editor.getComponent(Modal).props('open')).toBe(false)
      expect(prompt.getComponent(Modal).props('open')).toBe(true)
      await click(prompt, messages.devices.pairingRefuse)
      await click(prompt, messages.devices.pairingClose)
      expect(editor.getComponent(Modal).props('open')).toBe(true)
      expect(editor.get('input').element.value).toBe('D:/chosen/source.synra')
    }
    finally { prompt.unmount() }
  })
})
