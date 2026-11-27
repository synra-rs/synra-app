import type { SubscriptionOptions, Unsubscribe } from '@synra/client'
import type { PluginEventEnvelope, SynraError } from '@synra/protocol'
import type { InjectionContext } from '../context'
import type { GrantCheck } from '../grants'
import { createSynraError } from '../../errors'
import { asSynraError, denied, invalidArgument } from '../errors'
import { decodeEnvelope, DEVICE_TOPIC } from './decode'
import { compileEventPattern } from './pattern'

interface Subscription {
  readonly id: number
  readonly matches: (name: string) => boolean
  readonly handler: (payload: unknown) => void
  readonly onError: SubscriptionOptions['onError']
  phase: 'attaching' | 'active' | 'disposed'
  stream: string | undefined
}

/** One receive loop for all logical subscriptions on this page. */
export class PageSubscriptions {
  private readonly subscriptions = new Set<Subscription>()
  private nextId = 0
  private disposed = false
  private receiving = false

  constructor(private readonly context: InjectionContext, private readonly can: GrantCheck) {}

  subscribe<T>(pattern: string, handler: (payload: T) => void, options?: SubscriptionOptions): Unsubscribe {
    if (this.disposed)
      throw createSynraError('capability_denied', { reason: 'the page is closed' })
    const matches = compileEventPattern(pattern)
    if (matches === undefined)
      throw invalidArgument('invalid event pattern')
    if (matches(DEVICE_TOPIC) && !this.can('device:list'))
      throw denied('device:list')
    const subscription: Subscription = {
      id: ++this.nextId,
      matches,
      handler: payload => handler(payload as T),
      onError: options?.onError,
      phase: 'attaching',
      stream: undefined
    }
    this.subscriptions.add(subscription)
    void this.attach(subscription, pattern)
    return () => this.stop(subscription)
  }

  dispose(): void {
    this.disposed = true
    for (const subscription of this.subscriptions)
      this.stop(subscription)
  }

  private async call(method: string, args: Record<string, unknown>): Promise<unknown> {
    return this.context.bridge.invoke('plugin_ui_rpc', { token: this.context.token, method, args })
  }

  private async attach(subscription: Subscription, pattern: string): Promise<void> {
    try {
      const stream = await this.call('event.subscribe', { subscriptionId: subscription.id, pattern })
      if (typeof stream !== 'string' || stream.length === 0)
        throw createSynraError('protocol_violation', { reason: 'invalid event stream identifier' })
      subscription.stream = stream
      if (subscription.phase === 'disposed') {
        this.detach(subscription)
        return
      }
      subscription.phase = 'active'
      this.startReceiving()
    }
    catch (cause) {
      if (subscription.phase !== 'disposed') {
        this.stop(subscription)
        this.report(subscription, asSynraError(cause))
      }
    }
  }

  private stop(subscription: Subscription): void {
    if (subscription.phase === 'disposed')
      return
    subscription.phase = 'disposed'
    this.subscriptions.delete(subscription)
    this.detach(subscription)
  }

  private detach(subscription: Subscription): void {
    const streamId = subscription.stream
    if (streamId === undefined)
      return
    subscription.stream = undefined
    void this.call('event.unsubscribe', { streamId, subscriptionId: subscription.id }).catch((cause: unknown) => {
      if (!this.disposed)
        this.report(subscription, asSynraError(cause))
    })
  }

  private active(stream?: string): Subscription[] {
    return [...this.subscriptions].filter(subscription =>
      subscription.phase === 'active' && (stream === undefined || subscription.stream === stream))
  }

  private startReceiving(): void {
    if (this.receiving)
      return
    this.receiving = true
    void this.receive().finally(() => {
      this.receiving = false
      if (this.active().length > 0)
        this.startReceiving()
    })
  }

  private async receive(): Promise<void> {
    let cursor: { stream: string, seq: number } | undefined
    for (;;) {
      const stream = this.active()[0]?.stream
      if (stream === undefined)
        return
      if (cursor?.stream !== stream)
        cursor = { stream, seq: 0 }
      try {
        const raw = await this.call('event.poll', { streamId: stream })
        if (raw === null || this.active(stream).length === 0)
          continue
        let frame = decodeEnvelope(raw)
        if (frame.seq <= cursor.seq)
          continue
        if (frame.seq !== cursor.seq + 1) {
          const snapshot = await this.call('event.resync', { streamId: stream })
          if (this.active(stream).length === 0)
            continue
          cursor.seq = frame.seq
          if (snapshot === null)
            continue
          frame = decodeEnvelope(snapshot)
          if (frame.seq <= cursor.seq)
            throw createSynraError('protocol_violation', { reason: 'event snapshot did not advance the cursor' })
        }
        cursor.seq = frame.seq
        this.deliver(stream, frame)
      }
      catch (cause) {
        const error = asSynraError(cause)
        for (const subscription of this.active(stream)) {
          this.stop(subscription)
          this.report(subscription, error)
        }
      }
    }
  }

  private deliver(stream: string, frame: PluginEventEnvelope): void {
    for (const subscription of this.active(stream)) {
      if (subscription.phase !== 'active' || !subscription.matches(frame.name))
        continue
      try {
        subscription.handler(frame.payload)
      }
      catch (cause) {
        this.report(subscription, asSynraError(cause))
      }
    }
  }

  private report(subscription: Subscription, error: SynraError): void {
    if (subscription.onError === undefined) {
      console.error('Synra event subscription', error)
      return
    }
    try {
      subscription.onError(error)
    }
    catch (cause) {
      console.error('Synra event error handler', asSynraError(cause))
    }
  }
}
