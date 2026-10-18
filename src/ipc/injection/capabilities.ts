/**
 * What the host can actually do right now, and what it cannot.
 *
 * # Why this is one table and not two lists
 *
 * Because a host that advertises `event.subscribe` and then rejects it is worse than one that never claimed it. A plugin
 * author checks {@link EnvNamespace.hostSupports} to decide whether to use an API at all; if the answer is wrong the
 * failure lands on a user rather than in a review. So there is one table, the refusals read from it, and the advertised
 * lists are derived from it — there is no second place where "what works" is written down.
 *
 * The failure this avoids: a host whose `hostSupports('event.subscribe')` says yes while `subscribe` rejects every
 * pattern. The author writes the branch, ships it, and the user gets an error on a supported-looking path. Two lists
 * always drift; one table cannot.
 */

import type { ClientFeature } from '@synra/client'

export const BACKED: Readonly<Record<ClientFeature, boolean>> = {
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
  // ★ True since outbound requests went over the plugin channel: the host's own fetcher enforces the
  // `network:fetch:<host>` grants, so the call either reaches the network the user allowed or is refused by name.
  'network.fetch': true,
  // ★ True since the clipboard went over the plugin channel: the call reaches `platform:read-clipboard` and the
  // platform runner. An empty clipboard is `''`, not a refusal — the ordinary case must not be a `catch`.
  'clipboard.read': true
}
