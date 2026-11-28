// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { answer, IDENTITY, messages, mocks, mountPage, openDiagnostic } from './settings.testkit'

describe('settings diagnostics', () => {
  it('draws the metrics the recorder holds, keyed the way the kernel names them', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: {
              name: 'host_calls_total',
              labels: [
                ['plugin', '@acme/hello']
              ]
            },
            value: { kind: 'counter', value: 7 }
          },
          {
            key: { name: 'host_call_ms', labels: [] },
            value: {
              kind: 'histogram',
              value: {
                bounds: [1, 10],
                buckets: [2, 3, 1],
                count: 6,
                sum: 30
              }
            }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1000
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain('host_calls_total{plugin=@acme/hello}')
    expect(wrapper.text()).toContain('7')
    expect(wrapper.text()).toContain(messages.settings.metrics.histogram(6, '5.00'))
    wrapper.unmount()
  })
  it('polls metrics only while the performance section is visible', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY
    })
    vi.useFakeTimers()
    const wrapper = await mountPage()
    try {
      await flushPromises()
      expect(mocks.call).not.toHaveBeenCalledWith('diag_metrics')
      await openDiagnostic(wrapper, messages.settings.metrics.title)
      const header = wrapper.findAll('[data-testid="settings-diagnostics"] .ant-collapse-header').find(value => value.text().includes(messages.settings.metrics.title))!
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(1)
      await vi.advanceTimersByTimeAsync(5000)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(2)
      await header.trigger('click')
      await vi.advanceTimersByTimeAsync(10000)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'diag_metrics')).toHaveLength(2)
    }
    finally {
      wrapper.unmount()
      vi.useRealTimers()
    }
  })
  it('compares the whole start against the cold-start budget', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: {
              name: 'startup_duration_ms',
              labels: [
                ['phase', 'state']
              ]
            },
            value: {
              kind: 'histogram',
              value: {
                bounds: [1],
                buckets: [1, 0],
                count: 1,
                sum: 40
              }
            }
          },
          {
            key: {
              name: 'startup_duration_ms',
              labels: [
                ['phase', 'total']
              ]
            },
            value: {
              kind: 'histogram',
              value: {
                bounds: [1, 10, 100, 1000],
                buckets: [0, 0, 0, 2, 0],
                count: 2,
                sum: 700
              }
            }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1000
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain(messages.settings.metrics.startup)
    expect(wrapper.text()).toContain('350 ms')
    expect(wrapper.text()).toContain(messages.settings.metrics.startupWithin)
    wrapper.unmount()
  })
  it('says when the snapshot is incomplete rather than drawing it as the whole', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: { name: 'host_calls_total', labels: [] },
            value: { kind: 'counter', value: 1 }
          }
        ],
        overflowedTotal: 3,
        maxSeriesPerMetric: 2
      }
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openDiagnostic(wrapper, messages.settings.metrics.title)
    await flushPromises()
    expect(wrapper.text()).toContain(messages.settings.metrics.overflow(3, 2))
    wrapper.unmount()
  })
})
