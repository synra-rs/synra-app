/**
 * The commands the shell registers, checked against the contract it claims to implement.
 *
 * # Why this test lives here and not in `synra-ipc`
 *
 * `crates/synra-ipc/src/command.rs` has a test named `every_command_the_design_document_lists_exists`, and its comment
 * used to say: "A command in the document and not here is one the front end calls and the shell does not have." That is
 * the failure worth catching. The test cannot see it: it compares the registry against a hand-written list in the same
 * file, so it compares two copies of one decision.
 *
 * The gap it misses is real. The registry declares the contract's commands while the shell registers a subset, and each
 * unregistered one waits with a reason in `NOT_YET_REGISTERED` — so a front end calling an unbuilt command reaches
 * nothing, which is the exact sentence the comment promised was covered.
 *
 * A test inside that crate cannot read this file: it lives in a different repository. So the half about the shell is
 * checked here, where the shell is. `scripts/check-shapes.ts` already reads the sibling core checkout for the same
 * reason, so the dependency is not new.
 *
 * # What makes this a check rather than a second copy
 *
 * The declared names are **read out of the contract** (`command.rs`) rather than restated. A list here would have the
 * same defect as the list there: two hand-maintained copies agreeing with each other while the shell drifts. The only
 * hand-written part is `REGISTERED`, which is the claim being tested, and `NOT_YET_REGISTERED`, which is the gap — both
 * of them in `command-inventory.ts`, so that the data can be read at one go and the assertions below are what is left.
 *
 * The assertions run in both directions, so neither a rename nor a removal passes:
 *
 * every name in `generate_handler!` must be a command the contract declares;
 * every name in `REGISTERED` must appear in `generate_handler!`;
 * every declared command must be either registered or listed as waiting, with a reason.
 */

import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

import { NOT_YET_REGISTERED, REGISTERED } from '@/ipc/command-inventory'
import { readIpcContract } from '../../scripts/ipc-contract'
import { readTauriWrappers } from '../../scripts/tauri-wrappers'

const HERE = dirname(fileURLToPath(import.meta.url))
const APP_ROOT = resolve(HERE, '..', '..')
const { path: CONTRACT_RS, source: contractSource } = readIpcContract(APP_ROOT)

/** The command names the contract declares, read from `command.rs`. */
function declaredInContract(): string[] {
  // The `commands!` macro body is one entry per line: `"name" => Domain, Interface, ReturnShape, bool;`. Matching the
  // quoted name at the start of a line keeps this from picking up the macro definition above it, where the name is a
  // `$name` metavariable rather than a literal.
  const names: string[] = []
  for (const line of contractSource.split('\n')) {
    const match = /^\s*"([a-z0-9_]+)"\s*=>/.exec(line)
    if (match?.[1] !== undefined)
      names.push(match[1])
  }

  // A parse that found nothing would make every assertion below pass by comparing empty sets, which is the failure mode
  // this whole file exists to remove. So it is an error, not an empty result.
  if (names.length === 0) {
    throw new Error(`no command names were parsed from ${CONTRACT_RS}; the table's format has changed`)
  }
  return names
}

/** Commands that have a dispatcher route, a Tauri handler and a wrapper. */
function registeredInShell(): string[] {
  return [...readTauriWrappers(APP_ROOT).keys()]
}

describe('the commands the shell registers', () => {
  it('registers exactly the names this test claims it does', () => {
    const actual = registeredInShell().sort()

    // Compared by name rather than by count, so a rename is caught rather than absorbed. A count passes when one
    // command is renamed and another added in the same change, which is the shape of a merge.
    expect(actual).toEqual([...REGISTERED].sort())
  })

  it('registers nothing the contract does not declare', () => {
    const declared = new Set(declaredInContract())

    for (const name of registeredInShell()) {
      expect(declared.has(name), `${name} is registered but the contract does not declare it`).toBe(true)
    }
  })

  it('accounts for every declared command as either registered or waiting', () => {
    // ★ The assertion that makes the gap visible rather than absent. Read from the contract, so a command added there
    // and forgotten here fails — which is what the test in `synra-ipc` says it does and cannot.
    const declared = declaredInContract()
    const accounted = new Set<string>([...REGISTERED, ...Object.keys(NOT_YET_REGISTERED)])

    const missing = declared.filter(name => !accounted.has(name))
    expect(missing, 'declared by the contract and neither registered nor listed as waiting').toEqual([])

    // And the other direction: a name here that the contract no longer declares is a stale entry.
    const declaredSet = new Set(declared)
    const stale = [...accounted].filter(name => !declaredSet.has(name))
    expect(stale, 'listed here but no longer declared by the contract').toEqual([])

    // Stated as a number as well, so a parser that started missing entries shows up as a count change rather than as a
    // set that happens to still be consistent.
    expect(declared.length).toBe(57)
  })

  it('gives every unregistered command a reason', () => {
    for (const [name, reason] of Object.entries(NOT_YET_REGISTERED)) {
      expect(reason.length, `${name} has an empty reason`).toBeGreaterThan(10)
    }
  })
})
