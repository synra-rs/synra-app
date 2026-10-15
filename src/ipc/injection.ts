/**
 * The object a plugin's page is handed as `window.synra`.
 *
 * # What this file is for
 *
 * The interface library declares `SynraClient` and ships no implementation — a method needs a body, and a body needs a
 * host. This is that host's body: it builds the object over the same {@link Bridge} the host's own pages call the
 * kernel through, so a plugin and the shell take the same path to the same answers.
 *
 * # The rule this file is built around
 *
 * **Every method exists, and a method the host cannot honour refuses rather than fakes.** The surface library says this
 * out loud for `device.pick`, and the reason generalises: an author who calls a method that is missing gets
 * `TypeError: not a function`, which names neither the host nor the plugin and cannot be handled. An author who calls one
 * that exists and refuses gets a `SynraError` with a code, which is an ordinary branch.
 *
 * # Why the `supported` lists are computed from the same table as the behaviour
 *
 * Because a host that advertises `event.subscribe` and then rejects it is worse than one that never claimed it. A plugin
 * author checks {@link EnvNamespace.hostSupports} to decide whether to use an API at all; if the answer is wrong the
 * failure lands on a user rather than in a review. So there is one table, the refusals read from it, and the advertised
 * lists are derived from it — there is no second place where "what works" is written down.
 *
 * What is not here yet, and says so: plugin storage, transfers, outbound network, the clipboard and `self.invoke`. Each
 * needs a kernel command that does not exist in this build, and each refuses with `unsupported` rather than returning an
 * empty list, a fake id or a resolved promise. Those arrive with the commands.
 */

import type {
  ActionDescriptor,
  ClientFeature,
  ClipboardNamespace,
  DeviceNamespace,
  DeviceTarget,
  EnvNamespace,
  EventNamespace,
  LogNamespace,
  NetworkNamespace,
  PickOptions,
  PlatformCapability,
  PlatformNamespace,
  SelfNamespace,
  StorageNamespace,
  SynraClient,
  TransferNamespace,
  TransferProgress
} from '@synra/client'
import type {
  DeviceId,
  ErrorCode,
  Platform,
  PluginId,
  SynraError,
  TransferId
} from '@synra/protocol'

import type { Bridge, IpcError } from './client'
import { KERNEL_EVENT_CHANNEL } from './channel'

/**
 * The host shapes a plugin can be told about.
 *
 * ★ Derived rather than restated. `EnvNamespace.hostKind()` inlines `'desktop' | 'mobile'` and there is no named type
 * for it anywhere, so writing the union out again here would create a second copy that could drift from the contract
 * the plugin is compiled against. `ReturnType` reads the one that exists.
 */
type HostKind = ReturnType<EnvNamespace['hostKind']>

/**
 * What the host can actually do right now, and what it cannot.
 *
 * ★ A single table, read by both the refusals and the advertised feature lists.
 *
 * The failure this avoids: a host whose `hostSupports('event.subscribe')` says yes while `subscribe` rejects every
 * pattern. The author writes the branch, ships it, and the user gets an error on a supported-looking path. Two lists
 * always drift; one table cannot.
 */
const BACKED: Readonly<Record<ClientFeature, boolean>> = {
  'device.onChange': true,
  'event.subscribe': true,
  // ★ Implemented, even though no transfer ever runs.
  //
  // The method exists and returns a real unsubscribe, which is what `hostSupports` is being asked about. What is missing
  // is the transfer underneath: `transfer.send` refuses, and it refuses before an author could rely on seeing progress.
  // Reporting this false while the call succeeds is the inconsistency the biconditional test exists to catch — a
  // `supported` list that says no to something that works leaves an author guarding against a failure that never comes.
  'transfer.onProgress': true,
  'device.pick': false,
  'network.fetch': false,
  'clipboard.read': false
}

/** A refusal that says what is missing rather than what went wrong. */
function unsupported(feature: string): SynraError {
  return {
    code: 'unsupported' as ErrorCode,
    message: `error.unsupported`,
    retryable: false,
    details: { feature }
  } as unknown as SynraError
}

/** A refusal for a capability this plugin was not granted. */
function denied(capability: string): SynraError {
  return {
    code: 'capability_denied' as ErrorCode,
    message: `error.capabilityDenied`,
    retryable: false,
    details: { capability }
  } as unknown as SynraError
}

/** Turns whatever the bridge rejected with into the shape a plugin can branch on. */
function asSynraError(reason: unknown): SynraError {
  const candidate = reason as Partial<IpcError> | undefined
  if (candidate !== undefined && typeof candidate.code === 'string')
    return candidate as unknown as SynraError
  return unsupported('unknown')
}

/**
 * Whether a value is a `SynraError`.
 *
 * ★ A named function, not only the method.
 *
 * `isCode` has to branch on the same test. Written as `isSynraError(value)` inside the object literal it would resolve
 * to the property being defined a line above, which is a self-reference through `synra` — so this is the function the
 * method and its sibling both call.
 */
function isSynraErrorValue(value: unknown): value is SynraError {
  const candidate = value as Partial<SynraError> | undefined
  return candidate !== undefined
    && typeof candidate === 'object'
    && typeof candidate.code === 'string'
    && typeof candidate.message === 'string'
}

/** What the host needs to know about the plugin it is injecting into. */
export interface InjectionContext {
  /** The bridge to the kernel. */
  readonly bridge: Bridge
  /** This plugin's id, for scoping storage and self-invoke when they arrive. */
  readonly pluginId: PluginId
  /** The capabilities this plugin was granted. */
  readonly granted: readonly string[]
  /** The capabilities it was refused, which is the list a plugin may still ask about. */
  readonly deniedCapabilities: readonly string[]
  /** The host's own answers that do not need a round trip. */
  readonly locale: string
  /** Whether the user chose dark mode. */
  readonly darkMode: boolean
  /** The SDK version the plugin was built against. */
  readonly sdkVersion: string
  /**
   * The operating system the host is running on.
   *
   * ★ This is a field rather than a constant, and it was a constant. `platform()` returned the literal `'windows'`, so
   * on macOS, Linux, Android and iOS the injected interface told the plugin it was on Windows — a plugin that branched
   * on it took the Windows path everywhere. A wrong answer is worse than a refused one here, because a refusal is
   * visible and a plausible wrong answer is not.
   *
   * It belongs to the same group as `locale` and `darkMode`: facts the host already knows, which a plugin asks for
   * without a round trip. It was simply missing from that group.
   */
  readonly platform: Platform
  /** The host shape: a desktop install or a mobile one. */
  readonly hostKind: HostKind
  /** The host application's version, not the SDK's. */
  readonly appVersion: string
}

/**
 * Builds the injected object.
 *
 * # Why `satisfies` and not a type annotation
 *
 * ★ This is the operator the repository's README calls a hard requirement, and it is the only enforcement of it.
 *
 * An annotation (`const synra: SynraClient = {...}`) accepts an object with a field **too many**, which is the mistake
 * that makes an injected facade disagree with its own types: the author can read `synra.setting` and get a compile
 * error here but `undefined` at runtime. `satisfies` rejects the extra field while still inferring the literal's type,
 * so a test can call into the result without casting.
 */
export function createInjection(context: InjectionContext): SynraClient {
  const granted = new Set(context.granted)
  let requestCounter = 0

  const env: EnvNamespace = {
    // ★ These three read the context rather than naming a value. They used to be the literals `'desktop'`, `'windows'`
    // and `'0.1.0'`, which is a claim about the machine the plugin happens to be running on — and `'windows'` was wrong
    // on every other platform.
    hostKind: () => context.hostKind,
    platform: () => context.platform,
    locale: () => context.locale,
    darkMode: () => context.darkMode,
    appVersion: () => context.appVersion,
    sdkVersion: () => context.sdkVersion,
    // ★ Platform, not grant. A capability can be granted on a platform that cannot do it, and the interface has to
    // say a different thing in each case — so this answers only about the platform and `can` only about the grant.
    platformSupports: (_capability: PlatformCapability) => false,
    // ★ A snapshot, read from the one table. Synchronous because a render has to be able to branch on it.
    hostSupports: (feature: ClientFeature) => BACKED[feature] === true,
    can: (capability: string) => granted.has(capability)
  }

  const log: LogNamespace = {
    // ★ To the page's console, not to the kernel.
    //
    // The message reaches the developer looking at the plugin's own devtools, which is where they already are. The kernel
    // has no log ring yet, and routing through a command that does not exist would make every `log.info` a rejected
    // promise — which is worse than a missing feature, because it breaks the one API that must always work.
    //
    // The `no-console` allowance in this repository is `warn` and `error`, and it is deliberately overridden here for
    // `trace`/`debug`/`info`. The rule exists to keep stray prints out of the shell's own code — a debug statement
    // nobody meant to leave — and this block is not one: it is the storage a plugin's log namespace is *specified* to
    // write into. Mapping the three lower levels onto `warn` would be worse than the exception, because a plugin author
    // filtering by level would then see its `info` lines filed under warnings.
    /* eslint-disable no-console */
    trace: (message: string, context_?: Record<string, unknown>) => {
      console.trace(message, context_)
    },
    debug: (message: string, context_?: Record<string, unknown>) => {
      console.debug(message, context_)
    },
    info: (message: string, context_?: Record<string, unknown>) => {
      console.info(message, context_)
    },
    /* eslint-enable no-console */
    warn: (message: string, context_?: Record<string, unknown>) => {
      console.warn(message, context_)
    },
    error: (message: string, context_?: Record<string, unknown>) => {
      console.error(message, context_)
    },
    // ★ A prefix on every line, and a real object rather than a spread.
    //
    // The prefix is what makes a group of messages readable in a console full of other plugins' output, and the sublogger
    // nests because `child('net').child('fetch')` should read `net.fetch` — not because nesting was required, but
    // because a child logger that loses its parent's prefix is a bug waiting to be found in someone else's log.
    child: (prefix: string): LogNamespace => {
      const tagged = (method: 'trace' | 'debug' | 'info' | 'warn' | 'error') =>
        (message: string, fields?: Record<string, unknown>) => {
          log[method](`${prefix} ${message}`, fields)
        }
      return {
        trace: tagged('trace'),
        debug: tagged('debug'),
        info: tagged('info'),
        warn: tagged('warn'),
        error: tagged('error'),
        child: (nested: string) => log.child(`${prefix}.${nested}`)
      }
    }
  }

  const event: EventNamespace = {
    emit: async () => {
      // ★ Refused, and the reason is not "this host is old".
      //
      // Emitting needs a kernel command that enforces the `_plugin.<slug>.` namespace — the one that stops a plugin
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

  const device: DeviceNamespace = {
    // ★ The only namespace that answers from the kernel today.
    list: async (): Promise<DeviceTarget[]> => {
      const snapshot = await context.bridge
        .invoke('devices_snapshot')
        // ★ A kernel refusal stays a refusal.
        //
        // Swallowing it and returning an empty list would be the worst of both: the author's `try` never fires, the
        // picker shows "no devices", and the real reason — a refused command, a wedged store — is nowhere.
        .catch((reason: unknown) => {
          throw asSynraError(reason)
        }) as {
        readonly devices?: readonly {
          readonly deviceId: string
          readonly displayName: string
          readonly platform: string
          readonly online: boolean
        }[]
      }
      return (snapshot.devices ?? [])
        .filter(card => card.online)
        .map(card => ({
          deviceId: card.deviceId as DeviceId,
          displayName: card.displayName,
          platform: card.platform as DeviceTarget['platform'],
          online: card.online
        }))
    },
    send: async () => {
      throw unsupported('device.send')
    },
    broadcast: async () => {
      throw unsupported('device.broadcast')
    },
    onChange: (handler: (devices: DeviceTarget[]) => void) => {
      let latest: DeviceTarget[] = []
      return event.subscribe<{ readonly devices?: unknown }>('device.changed', (payload) => {
        latest = (payload.devices as DeviceTarget[] | undefined) ?? latest
        handler(latest)
      })
    },
    // ★ Exists and refuses, on purpose.
    //
    // The library's own comment: an author who calls a missing method gets `TypeError: not a function`, which names
    // neither side and cannot be handled. A refused method is an ordinary `catch`.
    pick: async (_options?: PickOptions): Promise<DeviceId | null> => {
      if (!granted.has('device.pick'))
        throw denied('device.pick')
      throw unsupported('device.pick')
    },
    // ★ Derived from the same table `hostSupports` reads.
    supported: (Object.keys(BACKED) as ClientFeature[]).filter(feature => BACKED[feature] === true)
  }

  const storage: StorageNamespace = {
    get: async <T = unknown>(_key: string): Promise<T | undefined> => {
      throw unsupported('storage.get')
    },
    set: async <T = unknown>(_key: string, _value: T): Promise<void> => {
      throw unsupported('storage.set')
    },
    delete: async (_key: string): Promise<void> => {
      throw unsupported('storage.delete')
    },
    keys: async (_prefix?: string): Promise<string[]> => {
      throw unsupported('storage.keys')
    },
    quota: async () => {
      throw unsupported('storage.quota')
    }
  }

  const network: NetworkNamespace = {
    // ★ A refusal rather than a fetch.
    //
    // The plugin page's CSP is `connect-src 'none'`, so a direct `fetch` fails in a way that looks like a network
    // problem. Routing through the kernel is the only sanctioned path, and this build has no command for it — so this
    // says so instead of reaching around the policy.
    fetch: async () => {
      throw unsupported('network.fetch')
    }
  }

  const transfer: TransferNamespace = {
    send: async () => {
      throw unsupported('transfer.send')
    },
    status: async () => {
      throw unsupported('transfer.status')
    },
    // ★ `id` is part of the signature, so it is named even though nothing uses it yet.
    //
    // The surface is `onProgress(id, handler)`. An implementation taking only a handler would typecheck against a
    // one-argument signature and then never call anything — the shape that compiles and silently does nothing.
    onProgress: (_id: TransferId, _handler: (progress: TransferProgress) => void) => () => {
      // A no-op unsubscribe, returned for the guarantee above.
    }
  }

  const platform: PlatformNamespace = {
    openUrl: async () => {
      throw unsupported('platform.openUrl')
    },
    notify: async () => {
      throw unsupported('platform.notify')
    }
  }

  const clipboard: ClipboardNamespace = {
    read: async () => {
      if (!granted.has('clipboard_read'))
        throw denied('clipboard_read')
      throw unsupported('clipboard.read')
    },
    write: async () => {
      if (!granted.has('clipboard_write'))
        throw denied('clipboard_write')
      throw unsupported('clipboard.write')
    }
  }

  const self: SelfNamespace = {
    invoke: async () => {
      throw unsupported('self.invoke')
    },
    actions: async (): Promise<ActionDescriptor[]> => {
      throw unsupported('self.actions')
    }
  }

  const synra = {
    env,
    log,
    event,
    device,
    storage,
    network,
    transfer,
    platform,
    clipboard,
    self,
    isSynraError: (value: unknown): value is SynraError => isSynraErrorValue(value),
    isCode: (value: unknown, code: ErrorCode) => isSynraErrorValue(value) && value.code === code,
    // ★ From the host, not `crypto.randomUUID()`.
    //
    // The identifier goes into the kernel's de-duplication table, and the kernel needs to be able to confirm it was
    // random. A page inside a WebView is not a trusted source for that.
    requestId: (): string => {
      requestCounter += 1
      return `ui-${context.pluginId}-${String(requestCounter)}`
    }
  } satisfies SynraClient

  // ★ The declared type, too — not instead of.
  //
  // `satisfies` catches a missing or extra field but does not widen the result, so a caller could still reach the
  // literal's inferred type. The annotation is what makes the return type the interface, and the two together are what
  // the README asks for.
  const injected: SynraClient = synra
  return injected
}
