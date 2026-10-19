/**
 * What is installed, shared by every screen that shows a plugin.
 *
 * # Why this store exists now
 *
 * The plugins page held its own array and re-read the whole list whenever an event arrived. That worked, and it was the
 * reason the page could not show a change without asking the kernel again — the event carried the card, and the card was
 * thrown away. Applying it here means the page draws what arrived, and the only full read is the one at startup.
 *
 * # The contract, the same as the other two stores
 *
 * `apply` answers whether it recognised the event. `false` is not "nothing happened"; it is "I cannot place this", which
 * is the caller's cue to re-read. A `pluginRemoved` event does not exist yet — removal has no card to carry — so the page
 * re-reads after an uninstall instead.
 */

import type { KernelEvent, PluginSnapshot } from '@synra/protocol'
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const usePluginsStore = defineStore('plugins', () => {
  /** Installed plugins, in the order the kernel first reported them. */
  const plugins = ref<readonly PluginSnapshot[]>([])

  /** Replaces the whole picture, for the read after startup. */
  function applySnapshot(next: readonly PluginSnapshot[]): void {
    plugins.value = [...next]
  }

  /** Applies one event. Answers whether this store knew what it was. */
  function apply(event: KernelEvent): boolean {
    if (event.type !== 'pluginUpdated')
      return false
    upsert(event.payload)
    return true
  }

  /**
   * Replaces the card with the same identifier, or appends it.
   *
   * In place rather than "remove then push": the order is what the user sees, and a list that reshuffled itself when a
   * plugin was enabled would move every card the user was not looking at.
   */
  function upsert(card: PluginSnapshot): void {
    const at = plugins.value.findIndex(plugin => plugin.pluginId === card.pluginId)
    plugins.value = at < 0
      ? [...plugins.value, card]
      : plugins.value.map((plugin, index) => (index === at ? card : plugin))
  }

  /** Drops a card, for the one path with no event — a successful uninstall. */
  function forget(pluginId: string): void {
    plugins.value = plugins.value.filter(plugin => plugin.pluginId !== pluginId)
  }

  /** One plugin by identifier. */
  function plugin(pluginId: string): PluginSnapshot | undefined {
    return plugins.value.find(plugin => plugin.pluginId === pluginId)
  }

  /**
   * Applies a health change.
   *
   * Only the faulted rung changes the card's `state`: `degraded` and `overreaching` are health facts a card may show
   * beside "active", while `faulted` means every call is refused. The explanation is the ladder's own sentence, not one
   * this store invents.
   */
  function markHealth(pluginId: string, health: string, explanation: string): boolean {
    const at = plugins.value.findIndex(plugin => plugin.pluginId === pluginId)
    if (at < 0 || health !== 'faulted')
      return at >= 0
    plugins.value = plugins.value.map((plugin, index) => index === at
      ? { ...plugin, state: 'faulted', stateReason: explanation }
      : plugin)
    return true
  }

  return { apply, applySnapshot, forget, markHealth, plugin, plugins, upsert }
})
