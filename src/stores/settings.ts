/**
 * The configuration, shared by every screen that reads or writes it.
 *
 * # Why a store rather than the settings page's own copy
 *
 * The kernel now **announces** a change (`configUpdated`), and an announcement is only useful if something is listening.
 * A page that kept its own copy would show the values it read at mount while another window changed them — and the user
 * has no way to tell a stale field from a refused write.
 *
 * # What it holds
 *
 * The overrides, not the effective configuration: `values` is what somebody wrote, and a key absent from it is one the
 * kernel is deciding. A screen that displayed this as "the configuration" would present defaults as choices.
 */

import type { ConfigSnapshot, KernelEvent } from '@synra/protocol'
import type { CallResult, KernelClient } from '../ipc/client'
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { setLocalePreference } from '../locale'

export const useSettingsStore = defineStore('settings', () => {
  let revision = 0
  let latestRead = 0
  /** The keys somebody wrote, and their values. */
  const values = ref<Readonly<Record<string, unknown>>>({})

  /** Those keys, in the order the kernel listed them — what a "reset" affordance needs. */
  const overridden = ref<readonly string[]>([])

  /** Replaces the whole picture, for the read at startup. */
  function applySnapshot(snapshot: ConfigSnapshot): void {
    revision++
    // `values` is `unknown` on the wire — the Rust side is `serde_json::Value` — and the store's job is to hold the map, so
    // the narrowing happens here and once.
    values.value = { ...(snapshot.values as Record<string, unknown>) }
    overridden.value = [...snapshot.overridden]
    setLocalePreference(values.value['ui.locale'])
  }

  /** Ignore reads overtaken by a newer read or an authoritative configuration event. */
  async function read(client: Pick<KernelClient, 'call'>): Promise<CallResult<ConfigSnapshot>> {
    const ticket = ++latestRead
    const started = revision
    const answer = await client.call<ConfigSnapshot>('config_get')
    if (answer.ok && ticket === latestRead && started === revision)
      applySnapshot(answer.value)
    return answer
  }

  /** Applies one event. Answers whether this store knew what it was — the contract the other three stores share. */
  function apply(event: KernelEvent): boolean {
    if (event.type !== 'configUpdated')
      return false
    applySnapshot(event.payload)
    return true
  }

  /** One overridden key, or `undefined` when nobody has written it. */
  function value(key: string): unknown {
    return values.value[key]
  }

  return { apply, applySnapshot, overridden, read, value, values }
})
