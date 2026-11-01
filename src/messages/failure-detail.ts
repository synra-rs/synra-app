/**
 * The kernel's explanation for a refusal, from whichever key it used.
 *
 * # Why this is shared rather than written twice
 *
 * Two places need it and they see different shapes: `ErrorNotice.vue` is given an `IpcError`'s raw `params`, while
 * `pages/plugins/actions.ts` narrows a `CallResult` into a `Failure` before the notice ever runs. One copy of the rule
 * means the two cannot disagree about which keys count as prose.
 *
 * # What it reads, and what it will not
 *
 * `detail` and `reason` are the two the kernel writes sentences into — `pairing_connect` says "the selected device has
 * no discovery route", the id parsers say what would not parse, a service failure carries its own text. Every other key
 * in `params` is machine data: a command name, a raw id, a status. Printing all of it is what turns the interface's one
 * error surface back into a dump of internals (R34), which is why this is a short list and not a `JSON.stringify`.
 *
 * An empty or whitespace-only value counts as absent — a `detail: ""` is the kernel having nothing to add, and an empty
 * paragraph above the actions reads as a rendering bug.
 */
export function detailOf(params: unknown): string | undefined {
  if (typeof params !== 'object' || params === null)
    return undefined
  const record = params as Record<string, unknown>
  for (const key of ['detail', 'reason'] as const) {
    const value = record[key]
    if (typeof value === 'string' && value.trim() !== '')
      return value
  }
  return undefined
}
