/**
 * The three decisions the plugins page asks the kernel to make, apart from the page that draws them.
 *
 * # Why they are here rather than inline in the `.vue`
 *
 * Because the page's own tests would need a DOM — vitest is `environment: 'node'` and this repository has no component
 * harness — and the part worth asserting is **which command and which arguments**, not which button is where. These are
 * pure functions over a client, so a test drives them with a fake bridge and no DOM at all.
 *
 * # What is deliberately absent
 *
 * No state. Each call answers what the kernel said and nothing else; the page decides what to do with an answer (replace
 * a card, show a notice, keep a list). A helper that also mutated a store would be a second place where the page's state
 * is described.
 */

import type { InstallReport, PluginSnapshot, SuggestedAction, SyncPlanDto, SyncReport } from '@synra/protocol'

import type { KernelClient } from '../../ipc/client'

/** A refusal as the interface needs it: the code, the message **key** and the actions (R33). */
export interface Failure {
  readonly code: string
  readonly message: string
  readonly actions?: readonly SuggestedAction[] | undefined
}

/** What a call came to. */
export type Outcome<T>
  = | { readonly ok: true, readonly value: T }
    | { readonly ok: false, readonly failure: Failure }

/** Turns a failed `CallResult` into the shape the notice renders. */
function failureOf(error: { code: string, message: string, actions?: readonly SuggestedAction[] | undefined }): Failure {
  return { code: error.code, message: error.message, actions: error.actions }
}

/**
 * Installs the package in a directory.
 *
 * `source` is tagged rather than a bare string because the command's argument is an `InstallSourceDto`: `path` is the
 * only variant this shell can serve — a `registry` one is refused by name — and spelling it here keeps the tag in one
 * place instead of at every call site.
 */
export async function installFrom(client: KernelClient, path: string): Promise<Outcome<InstallReport>> {
  const answer = await client.call<InstallReport>('plugins_install', {
    source: { source: 'path', path }
  })
  return answer.ok ? { ok: true, value: answer.value } : { ok: false, failure: failureOf(answer.error) }
}

/** Installs a package the configured registry serves, by its plugin id. */
export async function installFromRegistry(
  client: KernelClient,
  pluginId: string
): Promise<Outcome<InstallReport>> {
  const answer = await client.call<InstallReport>('plugins_install', {
    source: { source: 'registry', pluginId }
  })
  return answer.ok ? { ok: true, value: answer.value } : { ok: false, failure: failureOf(answer.error) }
}

/**
 * Records what the user allowed.
 *
 * ★ The granted list is **copied**. The page passes a reactive array, and a proxy handed to `invoke` is serialised by
 * whatever the bridge does with it — a copy makes the request a value rather than a window onto state that may change
 * while the call is in flight.
 */
export async function decide(
  client: KernelClient,
  pluginId: string,
  granted: readonly string[]
): Promise<Outcome<PluginSnapshot>> {
  const answer = await client.call<PluginSnapshot>('plugins_consent_respond', {
    pluginId,
    granted: [...granted]
  })
  return answer.ok ? { ok: true, value: answer.value } : { ok: false, failure: failureOf(answer.error) }
}

/**
 * Turns a plugin on or off.
 *
 * One function rather than two commands at the call site, because the two names differ only by the boolean — and a
 * page that chose between them itself would be a second place where "enabled" is spelled out.
 */
export async function setEnabled(client: KernelClient, pluginId: string, enabled: boolean): Promise<Outcome<null>> {
  const answer = await client.call<null>(enabled ? 'plugins_enable' : 'plugins_disable', { pluginId })
  return answer.ok ? { ok: true, value: null } : { ok: false, failure: failureOf(answer.error) }
}

/**
 * Removes a plugin, and its data when the caller says so.
 *
 * `removeData` is passed through rather than defaulted here: the decision belongs to the confirmation the user sees, and
 * a default in two places is a default that can disagree with the sentence next to the button.
 */
export async function uninstall(client: KernelClient, pluginId: string, removeData: boolean): Promise<Outcome<null>> {
  const answer = await client.call<null>('plugins_uninstall', {
    pluginId,
    removeData
  })
  return answer.ok ? { ok: true, value: null } : { ok: false, failure: failureOf(answer.error) }
}

/**
 * Asks the paired devices what they hold, and answers what would have to be sent.
 *
 * The plan is a separate call from the run because it can be **read**: a device that is offline is in the plan with
 * `online: false`, and a page that ran without looking would send to nobody and report a screen of failures. The two
 * steps are the kernel's own split, and this module does not merge them.
 */
export async function planSync(client: KernelClient, pluginId: string): Promise<Outcome<SyncPlanDto>> {
  const answer = await client.call<SyncPlanDto>('plugins_sync_plan', { pluginId })
  return answer.ok ? { ok: true, value: answer.value } : { ok: false, failure: failureOf(answer.error) }
}

/** Sends the artifact the plan marked as needed, and answers what each device did. */
export async function runSync(client: KernelClient, plan: SyncPlanDto): Promise<Outcome<SyncReport>> {
  const answer = await client.call<SyncReport>('plugins_sync_run', { plan })
  return answer.ok ? { ok: true, value: answer.value } : { ok: false, failure: failureOf(answer.error) }
}
