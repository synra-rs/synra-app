/**
 * The plugin's own headless half, over the plugin channel.
 *
 * # Why this namespace matters more than its two methods suggest
 *
 * The UI half is the least trustworthy shape a plugin has: JavaScript in a WebView, which the kernel cannot interrupt.
 * The headless half is Wasm, which it can — audited, rate-limited and interruptible. So a plugin that needs a dangerous
 * capability is meant to reach it by asking its headless half, and `self.invoke` is the asking. The kernel checks that
 * the action belongs to this plugin before the call, which is what stops one page from naming another plugin's action.
 *
 * # Why there is no grant check here
 *
 * Because there is no capability this call needs: `self.invoke` is the UI half asking its own other half. What the
 * headless half may then do is checked by the gate on **that** call, against the grants the user gave it. A check here
 * would be a second opinion about a permission this page does not hold in the first place.
 */

import type { ActionDescriptor, SelfNamespace } from '@synra/client'

import type { InjectionContext } from './context'
import { asSynraError, unsupported } from './errors'

/** The self namespace, over the plugin channel. */
export function createSelfNamespace(context: InjectionContext): SelfNamespace {
  const call = <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>

  return {
    invoke: async <Args = unknown, Result = unknown>(action: string, args?: Args): Promise<Result> => {
      // ★ `args` is omitted rather than sent as `undefined` when the caller gave none: the kernel reads a missing
      // arguments field as `null`, which is what "this action takes nothing" means, and an explicit `undefined` does not
      // survive JSON.
      return call<Result>('self.invoke', args === undefined ? { action } : { action, args })
    },
    actions: async (): Promise<ActionDescriptor[]> => {
      const listed = await call<unknown>('self.actions')
      if (!Array.isArray(listed)) {
        // A kernel that answered something else is a contract violation, and reading `.length` off it later would be the
        // crash rather than the message. This is the one place the façade has to distrust the host's shape.
        throw unsupported('self.actions')
      }
      return listed as ActionDescriptor[]
    }
  }
}
