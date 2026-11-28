// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { formatTime } from '@/utils/locale-format'
import { answer, HomePage, messages, mocks, router } from './index.testkit'

describe('home page intent decisions and details', () => {
  it('lets the user accept an intent on a candidate device', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT1',
      traceId: '01HQ00000000000000000TRACE1',
      state: 'choosing',
      candidates: [
        {
          pluginId: '@synra/notes',
          title: 'Notes',
          deviceId: '11111111-1111-4111-8111-111111111111',
          online: true
        }
      ],
      startedAtMs: 1
    }
    answer({
      intent_list: [intent],
      intent_decide: null
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    await flushPromises()
    const accept = wrapper.findAll('button').find(button => button.text() === messages.home.received.accept)
    expect(accept, 'the accept action is missing').toBeDefined()
    const idShown = wrapper.findAll('span').find(span => span.attributes('title') === intent.candidates[0]!.deviceId)
    expect(idShown, 'the candidate id has no title').toBeDefined()
    await accept?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'accept', deviceId: intent.candidates[0]!.deviceId }
    })
    wrapper.unmount()
  })
  it('lets the user decline an intent while it is still matching', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT2',
      traceId: '01HQ00000000000000000TRACE2',
      state: 'matching',
      candidates: [],
      startedAtMs: 1
    }
    answer({
      intent_list: [intent],
      intent_decide: null
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    await flushPromises()
    expect(wrapper.text()).toContain(messages.home.received.states.matching)
    const decline = wrapper.findAll('button').find(button => button.text() === messages.home.received.decline)
    expect(decline, 'a share still matching cannot be refused').toBeDefined()
    await decline?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'decline', reason: null }
    })
    wrapper.unmount()
  })
  it.each([
    ['matching', 'processing'],
    ['choosing', 'processing'],
    ['confirming', 'processing'],
    ['dispatching', 'processing'],
    ['succeeded', 'success'],
    ['noHandler', 'warning'],
    ['failed', 'error'],
    ['timedOut', 'error'],
    ['refused', 'error'],
    ['cancelled', 'default']
  ])('colours a %s intent %s', async (state, colour) => {
    answer({
      intent_list: [
        {
          intentId: 'i1',
          traceId: 't1',
          state,
          candidates: [],
          startedAtMs: 1
        }
      ]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    try {
      await flushPromises()
      const label = (messages.home.received.states as Record<string, string>)[state]
      const tag = wrapper.findAllComponents({ name: 'ATag' }).find(candidate => candidate.text() === label)
      expect(tag, `no tag for ${state}`).toBeDefined()
      expect(tag?.props('color')).toBe(colour)
    }
    finally {
      wrapper.unmount()
    }
  })
  it('gives the details toggle a full-size target', async () => {
    answer({
      intent_list: [
        {
          intentId: 'i1',
          traceId: 't1',
          state: 'succeeded',
          candidates: [],
          startedAtMs: 1
        }
      ]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    try {
      await flushPromises()
      const toggle = wrapper.findAll('button').find(button => button.attributes('aria-expanded') !== undefined)
      expect(toggle, 'the details toggle is missing').toBeDefined()
      expect(toggle!.classes()).toContain('min-h-11')
      expect(wrapper.html()).toContain('md:min-h-8')
    }
    finally {
      wrapper.unmount()
    }
  })
  it('says how many times a dispatch has been retried, and when it gave up', async () => {
    const nextAtMs = Date.UTC(2026, 9, 22, 10, 42)
    const retrying = {
      intentId: '01HQ00000000000000000INTNT4',
      traceId: '01HQ00000000000000000TRACE4',
      state: 'dispatching',
      candidates: [],
      retry: {
        attempts: 3,
        maxAttempts: 8,
        gaveUp: false,
        nextAtMs
      },
      startedAtMs: 1
    }
    const gaveUp = {
      intentId: '01HQ00000000000000000INTNT5',
      traceId: '01HQ00000000000000000TRACE5',
      state: 'dispatching',
      candidates: [],
      retry: { attempts: 8, maxAttempts: 8, gaveUp: true },
      startedAtMs: 1
    }
    answer({
      intent_list: [retrying, gaveUp]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    await flushPromises()
    const clock = formatTime(nextAtMs, true)
    expect(wrapper.text()).toContain(`Retrying (3/8), next try at ${clock}`)
    expect(wrapper.text()).toContain(messages.home.received.noAnswer)
    wrapper.unmount()
  })
  it('says why an intent failed without printing the kernel key', async () => {
    const intent = {
      intentId: '01HQ00000000000000000INTNT3',
      traceId: '01HQ00000000000000000TRACE3',
      state: 'failed',
      candidates: [],
      error: { code: 'plugin_not_installed', message: 'error.somethingUnmapped', retryable: false },
      startedAtMs: 1
    }
    answer({
      intent_list: [intent]
    })
    const wrapper = mount(HomePage, {
      global: {
        plugins: [
          router()
        ]
      }
    })
    await flushPromises()
    expect(wrapper.text()).toContain(messages.errors.plugin_not_installed)
    expect(wrapper.text()).not.toContain('error.somethingUnmapped')
    wrapper.unmount()
  })
})
