import type { DeviceCard, TransferSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setLocalePreference } from '@/app/locale'
import { installBrowserApis } from '@/testing/browser'
import { formatTime } from '@/utils/locale-format'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))
vi.mock('@/app/kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))
const HomePage = (await import('@/pages/index.vue')).default
const { useIntentsStore } = await import('@/stores/intents')
const { useDevicesStore } = await import('@/stores/devices')
const { useNoticesStore } = await import('@/stores/notices')
const { INTENT, DEVICES, IDENTITY } = await import('@/ipc/fake-kernel-fixtures')
const { messages } = await import('@/messages')

function router(): ReturnType<typeof createRouter> {
  return createRouter({ history: createMemoryHistory(), routes: [] })
}
function answer(byCommand: Record<string, unknown>) {
  const answers: Record<string, unknown> = { transfer_list: [], transfer_received: [], ...byCommand }
  mocks.call.mockImplementation(async (command: string) => command in answers
    ? { ok: true, value: answers[command] }
    : { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } })
}
beforeEach(() => {
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => { })
})
afterEach(() => setLocalePreference('system'))
export { afterEach, answer, beforeEach, createMemoryHistory, createPinia, createRouter, describe, DEVICES, expect, flushPromises, formatTime, HomePage, IDENTITY, installBrowserApis, INTENT, it, messages, mocks, mount, router, setActivePinia, setLocalePreference, useDevicesStore, useIntentsStore, useNoticesStore, vi }
export type { DeviceCard, TransferSnapshot }
