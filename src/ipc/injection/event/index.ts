import type { EventNamespace } from '@synra/client'
import type { InjectionContext } from '../context'
import type { GrantCheck } from '../grants'
import { unsupported } from '../errors'
import { PageSubscriptions } from './subscriptions'

export function createEventNamespace(context: InjectionContext, can: GrantCheck, lifetime?: EventTarget): {
  namespace: EventNamespace
  subscriptions: PageSubscriptions
} {
  const subscriptions = new PageSubscriptions(context, can)
  lifetime?.addEventListener('pagehide', () => subscriptions.dispose(), { once: true })
  const namespace: EventNamespace = {
    emit: async () => {
      throw unsupported('event.emit')
    },
    subscribe: (pattern, handler, options) => subscriptions.subscribe(pattern, handler, options)
  }
  return { namespace, subscriptions }
}
