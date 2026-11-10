import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { readIpcContract } from '../../scripts/ipc-contract'

const SOURCE = 'commands! { "identity_get" => Identity, Read, One, false; }'
let workspace: string
let appRoot: string

function checkout(root: string, ...layout: string[]): string {
  const path = join(root, 'crates', 'synra-ipc', 'src', ...(layout.length === 0 ? ['command.rs'] : layout))
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, SOURCE)
  return path
}

beforeEach(() => {
  workspace = mkdtempSync(join(tmpdir(), 'synra-contract-'))
  appRoot = join(workspace, 'synra-app')
  mkdirSync(appRoot)
})

afterEach(() => {
  vi.unstubAllEnvs()
  rmSync(workspace, { recursive: true, force: true })
})

describe('the IPC contract checkout', () => {
  it('reads the sibling used for local development', () => {
    const path = checkout(join(workspace, 'synra-rust-core'))
    expect(readIpcContract(appRoot, '')).toEqual({ path, source: SOURCE })
  })

  it('prefers the catalogue in the selected checkout over an older flat file', () => {
    const root = join(workspace, 'synra-rust-core')
    checkout(root)
    const path = checkout(root, 'command', 'catalog.rs')
    expect(readIpcContract(appRoot, '')).toEqual({ path, source: SOURCE })
  })

  it('reads the checkout inside the app used by CI', () => {
    const path = checkout(join(appRoot, 'synra-rust-core'))
    expect(readIpcContract(appRoot, '')).toEqual({ path, source: SOURCE })
  })

  it('prefers an explicit kernel root over both default checkouts', () => {
    checkout(join(workspace, 'synra-rust-core'))
    checkout(join(appRoot, 'synra-rust-core'))
    const root = join(workspace, 'custom-core')
    const path = checkout(root)
    expect(readIpcContract(appRoot, root)).toEqual({ path, source: SOURCE })
  })

  it('reads SYNRA_CORE_ROOT when no root argument is supplied', () => {
    const root = join(workspace, 'custom-core')
    const path = checkout(root)
    vi.stubEnv('SYNRA_CORE_ROOT', root)
    expect(readIpcContract(appRoot)).toEqual({ path, source: SOURCE })
  })

  it('prefers the sibling when both default checkouts exist', () => {
    const path = checkout(join(workspace, 'synra-rust-core'))
    checkout(join(appRoot, 'synra-rust-core'))
    expect(readIpcContract(appRoot, '')).toEqual({ path, source: SOURCE })
  })

  it('continues past a directory that does not contain the contract', () => {
    mkdirSync(join(workspace, 'synra-rust-core'))
    const path = checkout(join(appRoot, 'synra-rust-core'))
    expect(readIpcContract(appRoot, '')).toEqual({ path, source: SOURCE })
  })

  it('fails with every searched path and setup instructions when no contract exists', () => {
    const root = join(workspace, 'custom-core')
    for (const expected of [
      'SYNRA_CORE_ROOT',
      join(root, 'crates', 'synra-ipc', 'src', 'command.rs'),
      join(workspace, 'synra-rust-core', 'crates', 'synra-ipc', 'src', 'command.rs'),
      join(appRoot, 'synra-rust-core', 'crates', 'synra-ipc', 'src', 'command.rs')
    ]) {
      expect(() => readIpcContract(appRoot, root)).toThrow(expected)
    }
  })
})
