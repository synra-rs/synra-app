import type { SubscriptionOptions, Unsubscribe } from '@synra/client'
import type { PluginEvent, PluginTransferStatus, SynraError, TransferId } from '@synra/protocol'
import type { InjectionContext } from '../context'
import type { GrantCheck } from '../grants'
import { createSynraError } from '../../errors'
import { asSynraError, denied, invalidArgument } from '../errors'
import { decodePoll, decodeRecovery, DEVICE_TOPIC, TRANSFER_TOPIC } from './decode'
import { compileEventPattern } from './pattern'

interface Subscription {
  readonly id: number
  readonly matches: (event: PluginEvent) => boolean
  readonly handler: (payload: unknown) => void
  readonly onError: SubscriptionOptions['onError']
  phase: 'attaching' | 'active' | 'disposed'
  stream: string | undefined
}

const TOPIC_GRANTS = [
  [DEVICE_TOPIC, 'device:list'],
  [TRANSFER_TOPIC, 'file:transfer:send']
] as const

const RECOVERY_DELAYS_MS = [100, 300] as const

/** One receive loop for all logical subscriptions on this page. */
export class PageSubscriptions {
  private readonly subscriptions = new Set<Subscription>()
  private nextId = 0
  private disposed = false
  private receiving = false

  constructor(private readonly context: InjectionContext, private readonly can: GrantCheck) {}

  subscribe<T>(pattern: string, handler: (payload: T) => void, options?: SubscriptionOptions): Unsubscribe {
    const matches = compileEventPattern(pattern)
    if (matches === undefined)
      throw invalidArgument('invalid event pattern')
    const required = TOPIC_GRANTS.filter(([name]) => matches(name))
    const first = required[0]
    if (first !== undefined && !required.some(([, grant]) => this.can(grant)))
      throw denied(first[1])
    return this.register({ pattern }, (frame) => {
      const rule = TOPIC_GRANTS.find(([name]) => frame.name === name)
      return matches(frame.name) && (rule === undefined || this.can(rule[1]))
    }, payload => handler(payload as T), options)
  }

  subscribeTransfer(id: TransferId, handler: (status: PluginTransferStatus) => void, initial: () => Promise<PluginTransferStatus>, options?: SubscriptionOptions): Unsubscribe {
    if (!this.can('file:transfer:send'))
      throw denied('file:transfer:send')
    const matches = (frame: PluginEvent): boolean => frame.name === TRANSFER_TOPIC && frame.payload.transferId === id
    return this.register({ transferId: id }, matches, payload => handler(payload as PluginTransferStatus), options, initial)
  }

  private register(
    args: { pattern: string } | { transferId: TransferId },
    matches: Subscription['matches'],
    handler: Subscription['handler'],
    options?: SubscriptionOptions,
    initial?: () => Promise<unknown>
  ): Unsubscribe {
    if (this.disposed)
      throw createSynraError('capability_denied', { reason: 'the page is closed' })
    const subscription: Subscription = {
      id: ++this.nextId,
      matches,
      handler,
      onError: options?.onError,
      phase: 'attaching',
      stream: undefined
    }
    this.subscriptions.add(subscription)
    void this.attach(subscription, args, initial)
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

  private async attach(subscription: Subscription, args: Record<string, unknown>, initial?: () => Promise<unknown>): Promise<void> {
    try {
      const stream = await this.call('event.subscribe', { ...args, subscriptionId: subscription.id })
      if (typeof stream !== 'string' || stream.length === 0)
        throw createSynraError('protocol_violation', { reason: 'invalid event stream identifier' })
      subscription.stream = stream
      if (subscription.phase === 'disposed') {
        this.detach(subscription)
        return
      }
      subscription.phase = 'active'
      this.startReceiving()
      if (initial !== undefined) {
        const snapshot = await initial()
        if (this.subscriptions.has(subscription))
          this.notify(subscription, snapshot)
      }
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
    let recoverAt: number | undefined
    let failures = 0
    for (;;) {
      const stream = this.active()[0]?.stream
      if (stream === undefined)
        return
      if (cursor?.stream !== stream) {
        cursor = { stream, seq: 0 }
        recoverAt = undefined
        failures = 0
      }
      try {
        if (recoverAt !== undefined) {
          cursor.seq = await this.recover(stream, recoverAt)
          recoverAt = undefined
          continue
        }
        const raw = await this.call('event.poll', { streamId: stream })
        failures = 0
        if (raw === null || this.active(stream).length === 0)
          continue
        const frame = decodePoll(raw)
        if ('resync' in frame) {
          recoverAt = cursor.seq
          continue
        }
        if (frame.seq <= cursor.seq)
          continue
        if (frame.seq !== cursor.seq + 1) {
          recoverAt = frame.seq
          continue
        }
        cursor.seq = frame.seq
        this.deliver(stream, frame)
      }
      catch (cause) {
        const error = asSynraError(cause)
        const delay = RECOVERY_DELAYS_MS[failures]
        if (error.retryable && delay !== undefined && this.active(stream).length > 0) {
          failures++
          recoverAt ??= cursor.seq
          await new Promise<void>(resolve => setTimeout(resolve, delay))
          continue
        }
        for (const subscription of this.active(stream)) {
          this.stop(subscription)
          this.report(subscription, error)
        }
      }
    }
  }

  private async recover(stream: string, minimum: number): Promise<number> {
    const raw = await this.call('event.resync', { streamId: stream })
    if (this.active(stream).length === 0)
      return minimum
    const snapshot = decodeRecovery(raw)
    if (snapshot.seq < minimum)
      throw createSynraError('protocol_violation', { reason: 'event recovery moved the cursor backwards' })
    for (const frame of snapshot.events)
      this.deliver(stream, frame)
    return snapshot.seq
  }

  private deliver(stream: string, frame: PluginEvent): void {
    for (const subscription of this.active(stream)) {
      if (subscription.phase !== 'active' || !subscription.matches(frame))
        continue
      this.notify(subscription, frame.payload)
    }
  }

  private notify(subscription: Subscription, payload: unknown): void {
    try {
      subscription.handler(payload)
    }
    catch (cause) {
      this.report(subscription, asSynraError(cause))
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
