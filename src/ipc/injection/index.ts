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
 *
 * # Why the surface is assembled here and the groups live beside it
 *
 * The file was one long one, and the owner's rule is that a source file should not be long. The public path is unchanged:
 * `./injection` still exports `createInjection`, the guards and the context type. Each group now has a file that owns one
 * job — the environment, the log, events, devices, the namespaces that refuse — and this file is only the assembly.
 */

import type { SynraClient } from '@synra/client'
import type { ErrorCode, SynraError } from '@synra/protocol'

import type { InjectionContext } from './context'
import { createDeviceNamespace } from './device'
import { createEnvNamespace } from './env'
import { isSynraErrorValue } from './errors'
import { createEventNamespace } from './event'
import { createLogNamespace } from './log'
import {
  createClipboardNamespace,
  createNetworkNamespace,
  createPlatformNamespace,
  createSelfNamespace,
  createStorageNamespace,
  createTransferNamespace
} from './unsupported-namespaces'

export type { HostKind, InjectionContext } from './context'

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

  const event = createEventNamespace(context)

  const synra = {
    env: createEnvNamespace(context, granted),
    log: createLogNamespace(),
    event,
    device: createDeviceNamespace(context, event, granted),
    storage: createStorageNamespace(),
    network: createNetworkNamespace(),
    transfer: createTransferNamespace(),
    platform: createPlatformNamespace(),
    clipboard: createClipboardNamespace(granted),
    self: createSelfNamespace(),
    isSynraError: (value: unknown): value is SynraError => isSynraErrorValue(value),
    isCode: (value: unknown, code: ErrorCode) => isSynraErrorValue(value) && value.code === code,
    // ★ The narrowing one, and it is not a spelling of `isCode`.
    //
    // `isCode` answers `boolean`, so a `catch` branch using it learns nothing about the value's type — an author would
    // have to assert the code a second time to read it. This one narrows to the literal, which is what makes
    // `if (synra.isCapabilityDenied(e))` useful inside the branch rather than only at the test.
    //
    // The design document listed it (§4.1) and both of its examples use it (§5.2, §8), while the implementation had
    // neither it nor `ready` — so an author copying the complete example got a method that does not exist. Found by
    // compiling that example (`synra-js` guard 8), not by reading either side.
    isCapabilityDenied: (value: unknown): value is SynraError & { code: 'capability_denied' } =>
      isSynraErrorValue(value) && value.code === 'capability_denied',
    /**
     * Tells the host the page has mounted.
     *
     * # Why the failure is swallowed here, and only here
     *
     * The contract is `ready(): void` — synchronous, and the page does not wait for an answer. `plugin_ui_ready` is a
     * real name in the kernel's command table (`Interface::Plugin`), so this is a call into the contract rather than an
     * invented one; what it is not yet is a command *this shell registers*, and a page cannot do anything about that.
     * Letting the rejection escape would produce an unhandled rejection in a plugin author's console, naming a command
     * they never called and cannot fix — which is worse than the truth, that the host is not listening yet.
     */
    ready: (): void => {
      void context.bridge.invoke('plugin_ui_ready').catch(() => undefined)
    },
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
