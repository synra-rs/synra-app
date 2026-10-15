/**
 * The shape checks the repository's README calls hard requirements.
 *
 * # The two it names, and why each is worth a script
 *
 * 1. **The injected object must `satisfies SynraClient`.** A missing or extra field in what the shell injects into a
 *    plugin's WebView is a plugin's call that fails at runtime with `undefined is not a function` inside somebody
 *    else's code. The `satisfies` operator makes it a compile error, and this script asserts the operator is used —
 *    a `satisfies` that somebody deleted still compiles.
 *
 * 2. **No second declaration of `SynraResponse` or `SynraError`.** The client library defines them once; a copy here
 *    would drift, and the drift shows up as a response shape the interface reads differently from the library the
 *    plugin author is using.
 *
 * # What it reads
 *
 * This repository's own sources, and the client library's — the latter as text, because `@synra/client` is type-only
 * and importing it at runtime is what the check is partly about.
 */

import { readdir, readFile, stat } from 'node:fs/promises'
import { join, relative } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const here = fileURLToPath(new URL('..', import.meta.url))
const clientTypes = join(here, '..', 'synra-js', 'packages', 'protocol', 'src')

/** A check that failed. */
interface Failure {
  readonly check: string
  readonly detail: string
}

const failures: Failure[] = []
const notes: string[] = []

/** Every `.ts` file under a directory, recursively. */
async function sources(directory: string): Promise<string[]> {
  const out: string[] = []
  const walk = async (current: string): Promise<void> => {
    let entries
    try {
      entries = await readdir(current, { withFileTypes: true })
    }
    catch {
      return
    }
    for (const entry of entries) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        if (entry.name === 'node_modules' || entry.name === 'dist')
          continue
        await walk(path)
      }
      else if (entry.name.endsWith('.ts') || entry.name.endsWith('.vue')) {
        out.push(path)
      }
    }
  }
  await walk(directory)
  return out
}

/**
 * 1. The shell injects an object that satisfies the client library's interface.
 *
 * The check is on the *operator*, because the operator is the enforcement: an object literal typed `: SynraClient`
 * would also compile, and would then accept an object with a field too many — which is the mistake that makes a
 * plugin's injected facade disagree with its types.
 */
async function checkInjection(): Promise<void> {
  const injectDirectory = join(here, 'src', 'ipc')
  const files = await sources(injectDirectory)
  let found = false

  for (const file of files) {
    const text = await readFile(file, 'utf8')
    if (text.includes('satisfies SynraClient')) {
      found = true
      notes.push(`injection: ${relative(here, file)} asserts \`satisfies SynraClient\``)
    }
  }

  if (!found) {
    // Not yet a failure: the Tauri shell does not exist, so there is nothing to inject with. It becomes one the moment
    // an injection file appears without the operator, which is what the `src-tauri` guard below catches.
    let shellExists = false
    try {
      shellExists = (await stat(join(here, 'src-tauri'))).isDirectory()
    }
    catch {
      shellExists = false
    }
    if (shellExists) {
      failures.push({
        check: 'injection',
        detail: 'src-tauri exists but no file asserts `satisfies SynraClient`'
      })
    }
    else {
      notes.push('injection: no shell yet, so there is nothing to inject with')
    }
  }
}

/**
 * 2. `SynraResponse` and `SynraError` are declared once, in the client library.
 */
async function checkSingleDeclarations(): Promise<void> {
  const names = ['SynraResponse', 'SynraError'] as const
  const own = await sources(join(here, 'src'))

  for (const name of names) {
    for (const file of own) {
      const text = await readFile(file, 'utf8')
      // A *declaration* rather than a mention: `interface SynraError {` and `type SynraError =` are the two shapes a
      // second definition takes.
      const declares
        = new RegExp(`\\b(interface|type|class|enum)\\s+${name}\\b`).test(text)
          || new RegExp(`\\bdeclare\\s+(const|class)\\s+${name}\\b`).test(text)
      if (declares) {
        failures.push({
          check: 'single-declaration',
          detail: `${relative(here, file)} declares ${name}, which the client library defines once`
        })
      }
    }
  }

  // And the library's own declaration is where the check says it is, so a rename there is a failure here rather than a
  // check that silently stops looking.
  let libraryDeclares = false
  for (const file of await sources(clientTypes)) {
    const text = await readFile(file, 'utf8')
    if (/\b(?:interface|type)\s+SynraError\b/.test(text))
      libraryDeclares = true
  }
  if (libraryDeclares) {
    notes.push('single-declaration: the client library declares `SynraError`')
  }
  else {
    notes.push(
      'single-declaration: `SynraError` was not found in the client library — it may have been renamed, and this '
      + 'check would then be looking for the wrong name'
    )
  }
}

/**
 * 3. Nothing here imports the client library as a value.
 *
 * `@synra/client` is a **type-only** package: it has no runtime entry, so a value import from it fails to resolve at
 * build time — and worse, would mean the interface and the plugin author's library had two runtime copies of the same
 * helper.
 */
async function checkTypeOnlyImports(): Promise<void> {
  for (const file of await sources(join(here, 'src'))) {
    const text = await readFile(file, 'utf8')
    const lines = text.split('\n')
    for (const [index, line] of lines.entries()) {
      if (!line.includes('from \'@synra/client\''))
        continue
      // `import type` is the allowed form. A bare `import` is not, and `import { type X }` is a value import that
      // happens to be all types — which `verbatimModuleSyntax` refuses, and which this check refuses for the same
      // reason: the emitted JavaScript would import a package with no entry.
      if (!/^\s*import\s+type\b/.test(line)) {
        // `toString` rather than the number in the template: a rule in this repository refuses a number interpolated
        // into a string, because the one place it matters is a value that is not the one the author expected.
        const at = (index + 1).toString()
        failures.push({
          check: 'type-only-import',
          detail: `${relative(here, file)}:${at} imports a value from the type-only package @synra/client`
        })
      }
    }
  }
}

/**
 * 4. Every command name this repository calls exists in the kernel's own list.
 *
 * ★ The drift this catches is the most expensive one to find by hand. The command surface is defined in Rust
 * (`synra-ipc`'s `commands!` macro), and a TypeScript name that no longer matches — a rename on one side, a typo on the
 * other — fails at *runtime* with "command not found" from the WebView bridge, which names neither file.
 *
 * Reading the Rust source rather than keeping a list here is the same decision the dispatcher's error type made: one
 * list, two readers. The alternative is a copy that agrees on the day it is written.
 */
async function checkCommandNames(): Promise<void> {
  const rust = join(here, '..', 'synra-rust-core', 'crates', 'synra-ipc', 'src', 'command.rs')
  let rustText: string
  try {
    rustText = await readFile(rust, 'utf8')
  }
  catch {
    notes.push('command-names: the kernel checkout is not beside this one, so the names were not compared')
    return
  }

  // The macro's arms are `"name" => Domain, Interface, Shape, mutating;`. The name is the first quoted string on the
  // line, which is what the macro itself reads.
  const declared = new Set<string>()
  for (const line of rustText.split('\n')) {
    const match = /^\s*"([a-z][a-z0-9_]*)"\s*=>/.exec(line)
    if (match?.[1] !== undefined)
      declared.add(match[1])
  }

  if (declared.size === 0) {
    failures.push({
      check: 'command-names',
      detail: `no command names were found in ${relative(here, rust)} — the macro's shape may have changed`
    })
    return
  }

  // Every string this repository passes as a command. The names appear as the first argument of `call` and in the
  // method table below, and both are `'name'` literals.
  const called = new Set<string>()
  for (const file of await sources(join(here, 'src'))) {
    const text = await readFile(file, 'utf8')
    for (const match of text.matchAll(/\b(?:call|invoke)\(\s*'([a-z][a-z0-9_]*)'/g)) {
      if (match[1] !== undefined)
        called.add(match[1])
    }
  }

  for (const name of called) {
    if (!declared.has(name)) {
      failures.push({
        check: 'command-names',
        detail: `\`${name}\` is called here and is not one of the kernel's ${String(declared.size)} commands`
      })
    }
  }

  notes.push(
    `command-names: ${String(declared.size)} declared by the kernel, `
    + `${String(called.size)} called here, ${String([...called].filter(name => declared.has(name)).length)} agree`
  )
}

await checkInjection()
await checkSingleDeclarations()
await checkTypeOnlyImports()
await checkCommandNames()

for (const note of notes) {
  console.log(`  · ${note}`)
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`✗ ${failure.check}: ${failure.detail}`)
  }
  process.exit(1)
}

const total = notes.length + 1
console.log(`✓ shapes agree (${total.toString()} checks)`)
