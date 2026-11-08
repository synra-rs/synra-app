// @vitest-environment jsdom
import type { PluginSnapshot, SyncPlanDto } from '@synra/protocol'
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

import PluginSync from '@/components/plugins/plugin-sync.vue'

type PluginProps = InstanceType<typeof PluginSync>['$props']
beforeEach(() => setActivePinia(createPinia()))

/**
 * `PluginSync` had no test.
 *
 * It is the one place in the plugin page with a real asynchronous lifecycle — open reads a plan, send runs it, and both
 * carry a revision number so a response that arrives after the panel moved on is dropped. None of that is observable
 * by looking at the screen, and every branch of it is silent when it works.
 *
 * The awkward cases are the ones that matter: a response arriving after the plugin's version changed, a close during
 * a send, and the focus handover. A happy-path test would pass just as well with all three removed.
 */

const PLAN: SyncPlanDto = {
  targets: [
    { deviceId: 'd1', displayName: 'Pixel 8', online: true, needsArtifact: true },
    { deviceId: 'd2', displayName: 'Tablet', online: false, needsArtifact: false }
  ]
} as unknown as SyncPlanDto

/** The smallest object that satisfies how the component uses `operations`. */
function operations(run: (id: string, kind: string) => Promise<unknown>) {
  return {
    busy: new Map<string, string>(),
    run
  } as unknown as PluginProps['operations']
}

function snapshot(overrides: Partial<PluginSnapshot> = {}): PluginSnapshot {
  return {
    pluginId: 'demo',
    title: 'Demo',
    version: '1.0.0',
    state: 'running',
    enabled: true,
    hasUi: false,
    requested: [],
    granted: [],
    ...overrides
  } as PluginSnapshot
}

afterEach(() => {
  vi.restoreAllMocks()
})

async function mountSync(run: (id: string, kind: string) => Promise<unknown>) {
  const wrapper = mount(PluginSync, {
    props: { plugin: snapshot(), operations: operations(run) },
    attachTo: document.body
  })
  await flushPromises()
  return wrapper
}

it('reads the plan when it opens, and offers to send when a target needs the artifact', async () => {
  const run = vi.fn(async (_id: string, kind: string) =>
    kind === 'syncPlan' ? { ok: true, value: PLAN } : { ok: true, value: {} }
  )
  const wrapper = await mountSync(run)
  try {
    await wrapper.find('button').trigger('click')
    await flushPromises()

    expect(run.mock.calls[0]?.slice(0, 2), 'the plan was not read').toEqual(['demo', 'syncPlan'])
    const targets = new DOMWrapper(document.body).find('[data-sync-targets]')
    expect(targets.exists(), 'the plan was not rendered').toBe(true)
    expect(targets.text()).toContain('Pixel 8')
    // One online target needs the artifact, so sending is meaningful.
    const start = new DOMWrapper(document.body).findAll('button').find(button => button.text() === 'Start sync')
    expect(start?.attributes('disabled'), 'send should be offered when a target needs the artifact').toBeUndefined()
  }
  finally {
    wrapper.unmount()
  }
})

it('does not offer to send when no online target needs anything', async () => {
  // The plan is readable but there is nothing to do: sending would be a round trip that changes nothing.
  const run = vi.fn(async () => ({
    ok: true,
    value: { targets: [{ deviceId: 'd1', displayName: 'Pixel 8', online: true, needsArtifact: false }] }
  }))
  const wrapper = await mountSync(run)
  try {
    await wrapper.find('button').trigger('click')
    await flushPromises()

    const start = new DOMWrapper(document.body).findAll('button').find(button => button.text() === 'Start sync')
    expect(start, 'the send button should still be on screen').toBeDefined()
    expect(start!.attributes('disabled'), 'send must be disabled when nothing needs the artifact').toBeDefined()
  }
  finally {
    wrapper.unmount()
  }
})

it('drops a plan that arrives after the plugin version changed', async () => {
  // ★ The panel is open on 1.0.0 and a plan is in flight. The plugin updates to 2.0.0 before the answer lands — the
  //   answer describes a version that is no longer installed. Applying it would show "up to date" for the wrong build.
  let resolvePlan: ((value: unknown) => void) | undefined
  const run = vi.fn(async (_id: string, kind: string) => {
    if (kind !== 'syncPlan')
      return { ok: true, value: {} }
    return new Promise((resolve) => {
      resolvePlan = resolve
    })
  })
  const wrapper = await mountSync(run)
  try {
    await wrapper.find('button').trigger('click')
    await flushPromises()

    await wrapper.setProps({ plugin: snapshot({ version: '2.0.0' }) })
    await flushPromises()

    resolvePlan?.({ ok: true, value: PLAN })
    await flushPromises()

    expect(new DOMWrapper(document.body).find('[data-sync-targets]').exists(), 'a plan for the old version was applied').toBe(false)
    // The reader is told why there is nothing to trust rather than being shown an empty panel.
    expect(document.body.textContent).toContain('Refresh')
  }
  finally {
    wrapper.unmount()
  }
})

it('keeps the panel shut while a send is in flight', async () => {
  // A close mid-send would leave the report unread and the operation running with no way to see it finish.
  //
  // `sending` is read from `operations.busy`, not from whether `run` has resolved — so the busy entry is what has to be
  // set here. A mock that only returns a pending promise leaves `sending` false and the test would pass on a component
  // that never checked it.
  // ★ `operations.ts` builds this as `reactive(new Map())`, and that is what makes `sending` a live computed. A plain
  //   `Map` in the mock would leave every derived value frozen at its first read — the test would then pass on a
  //   component that never consulted `busy` at all.
  const busy = reactive(new Map<string, string>())
  const run = vi.fn(async (_id: string, kind: string) => {
    if (kind === 'syncPlan')
      return { ok: true, value: PLAN }
    busy.set('demo', 'syncRun')
    return new Promise(() => {})
  })
  const wrapper = mount(PluginSync, {
    props: {
      plugin: snapshot(),
      operations: { busy, run } as unknown as PluginProps['operations']
    },
    attachTo: document.body
  })
  try {
    await flushPromises()
    await wrapper.findAll('button')[0]!.trigger('click')
    await flushPromises()

    const start = new DOMWrapper(document.body).findAll('button').find(button => button.text() === 'Start sync')!
    await start.trigger('click')
    await flushPromises()

    const modal = wrapper.findComponent({ name: 'AModal' })
    expect(modal.props('closable'), 'a send was closable').toBe(false)
    expect(modal.props('maskClosable'), 'a send could be dismissed by the mask').toBe(false)
    expect(modal.props('keyboard'), 'a send could be dismissed with Escape').toBe(false)
  }
  finally {
    wrapper.unmount()
  }
})
