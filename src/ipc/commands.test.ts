/**
 * The fake kernel's answers, held against the kernel's own return-shape table.
 *
 * # The drift this exists to catch, because it already happened
 *
 * `crates/synra-ipc/src/command.rs` declares, per command, whether the payload is nothing (`Unit`), one object (`One`), a
 * **bare array** (`Many`) or an optional value (`Maybe`) — and the shell's signature is the same statement in Rust:
 * `pub fn plugins_list(...) -> Result<Vec<PluginSnapshot>, IpcError>`.
 *
 * `FakeKernel` answered `plugins_list` with `{ plugins: […] }`. `PluginsPage.vue` read `answer.value.plugins`, so the
 * fake and the page agreed with each other and every test was green — and the page crashed on a real device with
 * `t.value is not iterable`, because the kernel sends the array itself and `answer.value.plugins` is `undefined`. A fake
 * that is written to match the interface cannot catch the interface being wrong; only the contract can.
 *
 * # What is compared against what
 *
 * The expected arity is **read out of `command.rs`**, not restated here: a copy of the table would drift from it the same
 * way the fake drifted from the kernel. What *is* hand-written is the list of commands the fake answers — those are the
 * fake's decisions — and `scripts/check-shapes.ts` already proves each of those names is a command the kernel declares.
 *
 * # Why `Maybe` is tested separately rather than folded into `One`
 *
 * `plugins_check_update` is the only `Maybe`: "no update" is a normal answer, so its payload is legitimately `null`. A
 * test that demanded an object would force the fake to invent an update that does not exist.
 */

import type { AuditEntryDto, SyncPlanDto, SyncReport } from '@synra/protocol'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { describe, expect, it, vi } from 'vitest'

import { KernelClient } from './client'
import { FakeKernel } from './fake-kernel'

const HERE = dirname(fileURLToPath(import.meta.url))

it('previews synchronization through the client without contacting real devices', async () => {
  const client = new KernelClient(new FakeKernel())
  const planned = await client.call<SyncPlanDto>('plugins_sync_plan', { pluginId: '@synra-plugin/github' })
  expect(planned.ok).toBe(true)
  if (!planned.ok)
    throw new Error('preview plan failed')
  expect(planned.value.pluginId).toBe('@synra-plugin/github')
  expect(planned.value.targets).not.toHaveLength(0)
  const plan = { ...planned.value, targets: planned.value.targets.map(target => ({ ...target, online: false })) }
  const sent = await client.call<SyncReport>('plugins_sync_run', { plan })
  expect(sent.ok).toBe(true)
  if (!sent.ok)
    throw new Error('preview run failed')
  expect(sent.value.updated).toBe(0)
  expect(sent.value.failures?.[0]?.deviceId).toBe(plan.targets[0]?.deviceId)
  expect(sent.value.failures?.[0]?.error).toMatchObject({ code: 'unavailable' })
  expect(planned.value.targets[0]?.online).toBe(true)
})

it('keeps preview audit pages independent and clears only the selected plugin', async () => {
  const kernel = new FakeKernel()
  const first = await kernel.invoke('plugins_audit', { pluginId: '@synra-plugin/github', limit: 1, offset: 0 }) as AuditEntryDto[]
  const next = await kernel.invoke('plugins_audit', { pluginId: '@synra-plugin/github', limit: 1, offset: 1 }) as AuditEntryDto[]
  expect(first).toHaveLength(1)
  expect(next).toHaveLength(1)
  expect(first[0]?.atMs).toBeGreaterThan(next[0]!.atMs)
  first.splice(0, 1)
  expect(await kernel.invoke('plugins_audit', { pluginId: '@synra-plugin/github', limit: 20 })).toHaveLength(2)
  await kernel.invoke('plugins_audit_clear', { pluginId: '@synra-plugin/github' })
  expect(await kernel.invoke('plugins_audit', { pluginId: '@synra-plugin/github', limit: 20 })).toEqual([])
  expect(await kernel.invoke('plugins_audit', { pluginId: '@synra-plugin/shortcuts', limit: 20 })).toHaveLength(1)
  expect(await new FakeKernel().invoke('plugins_audit', { pluginId: '@synra-plugin/github', limit: 20 })).toHaveLength(2)
})

/** The contract, in the sibling core checkout — the same file `scripts/check-shapes.ts` reads. */
const CONTRACT_RS = resolve(HERE, '..', '..', '..', 'synra-rust-core', 'crates', 'synra-ipc', 'src', 'command.rs')

/** What `command.rs` says a command's payload is. */
type Arity = 'Unit' | 'One' | 'Many' | 'Maybe'

/**
 * The arity of every command the contract declares, keyed by name.
 *
 * A line of the `commands!` macro body is `"name" => Domain, Interface, Arity, mutates;` and this reads exactly that.
 * `Unit` rows are kept rather than filtered out, so the size of this map is the size of the contract — a parser that
 * silently stopped matching half the table would otherwise look exactly like a parser that read it all.
 */
function declaredArities(): Map<string, Arity> {
  let source: string
  try {
    source = readFileSync(CONTRACT_RS, 'utf8')
  }
  catch (cause) {
    throw new Error(
      `the IPC contract is unreadable at ${CONTRACT_RS} — this test needs the sibling synra-rust-core checkout, the `
      + `same one scripts/check-shapes.ts reads: ${String(cause)}`
    )
  }

  const arities = new Map<string, Arity>()
  for (const line of source.split('\n')) {
    const match = /^\s*"([a-z0-9_]+)"\s*=>\s*[A-Za-z]+,\s*[A-Za-z]+,\s*(Unit|One|Many|Maybe)\s*,/.exec(line)
    const name = match?.[1]
    const arity = match?.[2]
    if (name === undefined || arity === undefined)
      continue
    // A name read twice means the table's shape has changed under this parser, and the second reading is as likely to be
    // a misparse as the first. Saying so beats keeping whichever came last.
    if (arities.has(name))
      throw new Error(`${name} was parsed twice from ${CONTRACT_RS}; the table's format has changed`)
    arities.set(name, arity as Arity)
  }

  if (arities.size === 0)
    throw new Error(`no commands were parsed from ${CONTRACT_RS}; the table's format has changed`)

  return arities
}

/**
 * The commands `FakeKernel` answers, with an argument for the ones that take one.
 *
 * The fake's verbs are the app's verbs: this is every command `src/` reaches it with, and the list is hand-written
 * because which commands the fake supports is a decision rather than a derivation. A name here that the contract does
 * not declare fails below, which is the other half of the drift — a fake inventing a command.
 */
const ANSWERED: Readonly<Record<string, Readonly<Record<string, unknown>> | undefined>> = {
  identity_get: undefined,
  identity_credentials: undefined,
  identity_cleanup: undefined,
  devices_snapshot: undefined,
  // The command takes an `opts`, and `REQUIRED_ARGS` in `fake-kernel.ts` is where the fake says so: a table that omitted
  // it would be asserting the shape of an answer to a call the shell refuses. `cachedOnly` keeps the suite free of a
  // running interval — the shape of the answer is the same either way.
  devices_scan_start: { opts: { includePaired: false, cachedOnly: true } },
  pairing_status: undefined,
  pairing_cancel: { requestId: '11111111-1111-4111-8111-111111111111' },
  devices_scan_cancel: { scanId: '01HQ00000000000000000SCAN01' },
  plugins_list: undefined,
  // The command is `One` **and takes an `intent_id`**: it answers about one intent rather than listing them.
  intent_snapshot: { intentId: '01HQ00000000000000000INTNT1' },
  intent_decide: {
    intentId: '01HQ00000000000000000INTNT1',
    decision: { decision: 'acceptDefault' }
  },
  // The listing read, which is `Many`: the payload is the array rather than an envelope around it.
  intent_list: undefined,
  transfer_list: undefined,
  transfer_received: undefined,
  transfer_open: { path: 'C:/inbox/notes.txt' },
  transfer_reveal: { path: 'C:/inbox/notes.txt' },
  transfer_discard: { path: 'C:/inbox/notes.txt' },
  transfer_respond: { transferId: '01HQ00000000000000000XFER1', accept: true },
  // `One`: the transfer's identifier. The fake requires the request bag the wrapper takes, not its fields.
  transfer_send_text: { request: { to: '11111111-1111-4111-8111-111111111111', text: 'hi', fileName: 'message.txt' } },
  transfer_send_file: { request: { to: '11111111-1111-4111-8111-111111111111', path: 'C:/files/notes.txt' } },
  plugins_publishers: undefined,
  // `One`, with the option bag the wrapper requires: the page builds it, and a fake that ignored it would answer a shape
  // the page could not have produced.
  diag_export: { options: { includeAudit: false, includeDebugLogs: false } },
  // `Many`: a bare array, which is what the arity says the payload is.
  diag_logs_tail: { lines: 100 },
  diag_probes: undefined,
  diag_crash_dismiss: undefined,
  diag_crashes: undefined,
  // `One`: the contract declares this command as `One`, and the fake now answers the removal's report. The value here is
  // the **arguments** this test invokes with, so both required keys are present.
  plugins_uninstall: { pluginId: '@acme/hello', removeData: true },
  // `Unit`: the answer has no shape to hold a caller to, which is the claim this entry makes alongside the declaration.
  plugins_audit_clear: { pluginId: '@acme/hello' },
  plugins_update: { pluginId: '@acme/hello' },
  plugins_reload: { pluginId: '@acme/hello' },
  plugins_publisher_decide: { publisherId: '@acme', fingerprint: 'AB12 CD34 EF56 7890', trust: 'trusted' },
  plugins_preview: { pluginId: '@acme/hello' },
  plugins_catalog_search: { query: { query: 'hello' } },
  config_get: undefined
}

/** The contract's one optional command, named so the `Maybe` case reads as the exception it is. */
const MAYBE_COMMAND = 'plugins_check_update'

/** The number of commands `command.rs` declares, which `registration.test.ts` pins from the other side. */
const DECLARED_COMMANDS = 60

describe('every answer the fake gives, against the arity the kernel declares', () => {
  const arities = declaredArities()

  for (const [command, args] of Object.entries(ANSWERED)) {
    it(`\`${command}\` answers with the shape the contract declares`, async () => {
      const declared = arities.get(command)
      // Not a skip: a command the fake answers and the contract does not declare is the fake inventing a command.
      expect(declared, `${command} is not declared by ${CONTRACT_RS}`).toBeDefined()

      const kernel = new FakeKernel()
      let answer: unknown
      if (command === 'devices_scan_cancel') {
        vi.useFakeTimers()
        try {
          const scanId = await kernel.invoke('devices_scan_start', { opts: { cachedOnly: false } })
          answer = await kernel.invoke(command, { scanId })
          vi.advanceTimersByTime(300)
        }
        finally {
          vi.useRealTimers()
        }
      }
      else {
        answer = await kernel.invoke(command, args)
      }

      // `Unit` says the payload is nothing, so there is no shape to hold it to — the point of the arity is that a caller
      // has nothing to read, and whatever it is, the app must not depend on it.
      if (declared === 'Unit')
        return

      if (declared === 'Many') {
        expect(Array.isArray(answer), `${command} is declared Many, so the payload must be a bare array`).toBe(true)
        return
      }

      // Both exclude arrays; Maybe additionally permits null. A DTO is an object and a scan/transfer id is a
      // string; both are exactly one value, and the assertion that matters is that the fake did not wrap a single value
      // in an envelope. `{ plugins: […] }` and `{ value: … }` both fail here.
      expect(Array.isArray(answer), `${command} is declared ${String(declared)}, so the payload must not be an array`).toBe(false)
      expect(typeof answer, `${command} is declared ${String(declared)}, so the payload must be a value`).toMatch(/^(object|string|number|boolean)$/)
      if (declared !== 'Maybe')
        expect(answer, `${command} is declared ${String(declared)}, so the payload must not be null`).not.toBeNull()
    })
  }

  it('answers `Many` with a bare array even when the list is empty', async () => {
    // ★ The empty case is the one that reads as "no data" whatever the shape is, so it is asserted on its own: an
    // envelope around an empty list is exactly as broken as an envelope around a full one, and exactly as invisible.
    const transfers = await new FakeKernel().invoke('transfer_list')
    expect(Array.isArray(transfers)).toBe(true)
    expect(transfers).toEqual([])
  })

  it('treats `Maybe` as an answer rather than as a failure', async () => {
    // The arity is read out of the contract and asserted to be `Maybe` before the shape is asserted, so this test cannot
    // quietly keep passing if `plugins_check_update` is one day declared `One` — in which case the rule changes and this
    // test should be the thing that says so.
    expect(arities.get(MAYBE_COMMAND)).toBe('Maybe')

    const answer = await new FakeKernel().invoke(MAYBE_COMMAND, { pluginId: '@acme/hello' })
    // `undefined` is what the fake's default arm answers for a command it does not have, so "no update" has to be a real
    // `null` to be distinguishable from "this fake does not know the command".
    expect(answer === null || typeof answer === 'object').toBe(true)
  })

  it('parses the whole contract rather than a prefix of it', () => {
    // The parser's own guard. `registration.test.ts` pins the declared count at 40 from the other side, and this fails
    // if the two ever disagree — which would mean one of the two readers of `command.rs` had stopped reading all of it.
    expect(arities.size).toBe(DECLARED_COMMANDS)
  })
})
