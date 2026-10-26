/**
 * The event namespace: one channel in, and an unsubscribe that is always a function.
 *
 * # Why nothing here is emittable
 *
 * Emitting needs a kernel command that enforces the `_plugin.<full @scope/name>.` namespace — the one that stops a plugin
 * impersonating another's events. Writing it here without that check would be an emitter anyone may use.
 */

import type { EventNamespace } from '@synra/client'

import type { InjectionContext } from './context'
import { KERNEL_EVENT_CHANNEL } from '../channel'
import { unsupported } from './errors'

/** The event namespace, over the bridge it was given. */
export function createEventNamespace(context: InjectionContext): EventNamespace {
  return {
    emit: async () => {
      // ★ Refused, and the reason is not "this host is old".
      //
      // Emitting needs a kernel command that enforces the `_plugin.<full @scope/name>.` namespace — the one that stops a plugin
      // impersonating another's events. Writing it here without that check would be an emitter anyone may use.
      throw unsupported('event.emit')
    },
    // ★ Always returns a function, even when nothing is listening.
    //
    // A `subscribe` that can return `undefined` teaches authors to write `maybe?.()` and to leak the subscription when
    // it does. The library's own comment calls this out, so the guarantee is made here rather than hoped for.
    subscribe: <T = unknown>(_pattern: string, handler: (payload: T) => void) => {
      let active = true
      void context.bridge
        .listen(KERNEL_EVENT_CHANNEL, (payload: unknown) => {
          if (active)
            handler(payload as T)
        })
        .then((off) => {
          if (active)
            return off
          // The caller unsubscribed before the listener was attached, so the one that just arrived is dropped here.
          off()
          return undefined
        })
        .catch(() => {
          // Nothing to report to: the plugin page's only log is the console, and an unsubscribe that failed to attach
          // is not something the author can do anything about.
        })
      return () => {
        active = false
      }
    }
  }
}
