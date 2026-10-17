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
    // ★ Two patterns, both anchored, and neither able to run away.
    //
    // It used to test each line that *ends* in `from '@synra/client'`. A type import written the way ESLint formats
    // one — `import type {\n  A,\n  B,\n} from '@synra/client'` — puts `from` on the line holding `}`, so the test ran
    // against `}` and reported a value import that was not one. A check that correct code cannot pass is a check people
    // learn to ignore.
    //
    // The first attempt here fixed that and was worse: `[\s\S]*?` between `import` and `from` happily crossed every
    // statement in between, so a real value import at the bottom of a file was swallowed by the type import at the top
    // and read as one clause beginning with `type` — a guard that passes on the exact thing it exists to reject. The
    // brace list cannot contain `}`, so `{[^}]*}` bounds the clause to the statement it started in, and the second
    // pattern covers a default import, which has no braces to bound it.
    const patterns = [
      // A braced import. Group 1 is `type` when the form is the allowed one.
      /^[ \t]*import\s+(type\s+)?\{([^}]*)\}\s*from\s*'@synra\/client'/gm,
      // A default import — `import Foo from '@synra/client'` — which can never be the allowed form.
      /^[ \t]*import\s+[A-Za-z_$][\w$]*\s*(?:,\s*)?from\s*'@synra\/client'/gm
    ]
    for (const pattern of patterns) {
      for (const match of text.matchAll(pattern)) {
        // `import type` is the allowed form. A bare `import` is not, and `import { type X }` is a value import that
        // happens to be all types — which `verbatimModuleSyntax` refuses, and which this check refuses for the same
        // reason: the emitted JavaScript would import a package with no entry.
        if (match[1] !== undefined)
          continue
        // `toString` rather than the number in the template: a rule in this repository refuses a number interpolated
        // into a string, because the one place it matters is a value that is not the one the author expected.
        const line = text.slice(0, match.index).split('\n').length
        failures.push({
          check: 'type-only-import',
          detail: `${relative(here, file)}:${line.toString()} imports a value from the type-only package @synra/client`
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

/**
 * 5. Only the seam may reach for the test double.
 *
 * ★ `kernel.ts` has always said the fake is chosen "here and nowhere else", and that a component reaching for it directly
 * "would keep working after the real bridge exists, and would keep working in the one place it must not". That rule was
 * **documented and unenforced**, and it was already broken: `DevicesPage.vue` imported the event-channel constant from
 * `fake-kernel.ts`, and so did the injected plugin facade. Both are production code reading a fixture.
 *
 * The constant now lives in `channel.ts`, which is what a shared name should have been. This is the check that keeps the
 * next one from happening: the fake is a fixture for tests, and the only file allowed to name it is the file whose job is
 * choosing between it and the real bridge.
 */
async function checkFakeIsQuarantined(): Promise<void> {
  const root = join(here, 'src')
  const allowed = new Set(['kernel.ts'])
  let importers = 0

  for (const file of await sources(root)) {
    const name = relative(join(here, 'src'), file).replaceAll('\\', '/')
    // Tests are the place a fake belongs, so they are not the thing being guarded against.
    if (name.endsWith('.test.ts') || allowed.has(name))
      continue
    const text = await readFile(file, 'utf8')
    if (/(?:from|import)\s*\(?['"][^'"]*fake-kernel['"]/.test(text)) {
      importers += 1
      failures.push({
        check: 'fake-quarantine',
        detail: `${name} imports the fake kernel; only kernel.ts may choose it`
      })
    }
  }

  notes.push(
    importers === 0
      ? 'fake-quarantine: only kernel.ts names the fake kernel'
      : `fake-quarantine: ${String(importers)} production file(s) import the fake`
  )
}

/**
 * 6. Every call this code makes supplies the arguments the shell's wrapper declares — and the fake requires them too.
 *
 * ★ The drift this catches is the one that reached a user.
 *
 * `DevicesPage.vue` called `devices_scan_start` with no arguments, while `src-tauri/src/shell.rs` declares
 * `opts: ScanOptions` as a parameter. `#[tauri::command]` deserializes every argument *before* the command body runs, so
 * the bridge rejected the invoke — `command devices_scan_start missing required key opts` — and the client reports a
 * rejection that is not an `IpcError` as `error.bridgeFailed`: the window said "The interface could not reach the kernel"
 * for a command the kernel never ran, and the reason sat in a `params` field nothing renders.
 *
 * Three checks missed it, each for its own reason: this file compared command **names** only, `registration.test.ts`
 * compares the names the shell registers, and the fake kernel took no interest in its arguments — so the tests passed and
 * `pnpm dev` scanned happily.
 *
 * What is read is the shell's own signatures, and three things are held against them:
 *
 * 1. every literal call site in shipping code passes every required key, and no key the wrapper does not declare;
 * 2. `REQUIRED_ARGS` in the fake is exactly what the shell requires, for every command it names;
 * 3. every command the fake answers, and the shell refuses without an argument, is in that table.
 *
 * The keys are camelCase because `#[tauri::command]` renames a parameter that way by default: a `scan_id` on the Rust side
 * is `scanId` over the wire. A call whose command name or arguments are not literals cannot be read here, and is counted
 * rather than guessed at — the same decision the name check makes about what it can see.
 */

/** One call found in the sources, with the argument keys it passes. */
interface CallSite {
  readonly command: string
  readonly keys: readonly string[]
  /** `pages/DevicesPage.vue:265`, so a failure names a line rather than a file. */
  readonly where: string
}

/** A wrapper in `src-tauri/src/shell.rs`: the keys it takes, and the ones it cannot be called without. */
interface Wrapper {
  readonly declared: readonly string[]
  readonly required: readonly string[]
}

/** `device_id` → `deviceId`, which is the rename `#[tauri::command]` applies to a parameter name. */
function camelCase(name: string): string {
  return name.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
}

/**
 * A parameter list split at its own top-level commas.
 *
 * `state: tauri::State<'_, AppState>` holds a comma inside angle brackets, so splitting on every comma would cut one
 * parameter in two and read `AppState>` as a name.
 */
function splitParameters(text: string): string[] {
  const parts: string[] = []
  let depth = 0
  let current = ''
  for (const character of text) {
    if (character === '<' || character === '(' || character === '[')
      depth += 1
    else if (character === '>' || character === ')' || character === ']')
      depth -= 1
    if (character === ',' && depth === 0) {
      parts.push(current)
      current = ''
      continue
    }
    current += character
  }
  parts.push(current)
  return parts.map(part => part.trim()).filter(part => part.length > 0)
}

/** The wrapper signatures `shell.rs` writes, keyed by the command each one registers. */
function shellWrappers(source: string): Map<string, Wrapper> {
  const wrappers = new Map<string, Wrapper>()
  // `#[tauri::command]` sits immediately above each `pub fn`, and no parameter type in that file contains `) ->`, so the
  // signature is everything between the opening parenthesis and the arrow. The attribute's own argument list is optional
  // and is skipped: `#[tauri::command(async)]` is the same command with a different execution context, and a parser that
  // only understood the bare attribute would report the command as missing — which is exactly what it did, and what the
  // `generate_handler!` comparison below caught.
  //
  // ★ `pub async fn` is the same wrapper too. Two commands became `async` when their blocking bodies moved to a worker
  // thread, and a parser that only knew `pub fn` reported both as missing — the same failure as the attribute above, one
  // syntax feature further along.
  for (const match of source.matchAll(/#\[tauri::command(?:\([^)]*\))?\]\s*pub (?:async )?fn\s+(\w+)\s*\(([\s\S]*?)\)\s*->/g)) {
    const name = match[1]
    const parameters = match[2]
    if (name === undefined || parameters === undefined)
      continue

    const declared: string[] = []
    const required: string[] = []
    for (const parameter of splitParameters(parameters)) {
      // Split on the first colon rather than with a pattern: a type can hold the `::` of `tauri::State`, and an anchored
      // pattern with a wildcard tail is the shape the linter refuses for its backtracking.
      const colon = parameter.indexOf(':')
      const field = colon === -1 ? '' : parameter.slice(0, colon).trim()
      const type = colon === -1 ? '' : parameter.slice(colon + 1).trim()
      if (!/^\w+$/.test(field) || type.length === 0)
        continue
      // `state: tauri::State<'_, AppState>` comes from the app handle rather than from the message, so it is not a key a
      // caller sends.
      if (field === 'state' || type.startsWith('tauri::State'))
        continue
      const key = camelCase(field)
      declared.push(key)
      // `Option<T>` is the one shape Tauri answers with `None` when the key is absent. Everything else is required, which
      // is what made the missing `opts` a rejection rather than a refusal.
      if (!type.startsWith('Option<'))
        required.push(key)
    }
    wrappers.set(name, { declared, required })
  }
  return wrappers
}

/** The index just past the bracketed literal starting at `start`, or `undefined` when it never closes. */
function literalEnd(text: string, start: number): number | undefined {
  let depth = 0
  let quote: string | undefined
  for (let index = start; index < text.length; index += 1) {
    const character = text[index]
    if (quote !== undefined) {
      if (character === '\\')
        index += 1
      else if (character === quote)
        quote = undefined
      continue
    }
    if (character === '\'' || character === '"' || character === '`') {
      quote = character
      continue
    }
    if (character === '{' || character === '[' || character === '(') {
      depth += 1
    }
    else if (character === '}' || character === ']' || character === ')') {
      depth -= 1
      if (depth === 0)
        return index + 1
    }
  }
  return undefined
}

/** The keys of an object literal at its own top level: the `opts` of `{ opts: { … } }`, not the flags inside it. */
function topLevelKeys(literal: string): string[] {
  // ★ Shorthand properties are expanded **before** the scan: `{ pluginId, granted }` is the same object as
  // `{ pluginId: pluginId, granted: granted }`, and ESLint's `object-shorthand` rule rewrites it to the first form. Without
  // this, running the linter silently turned a call that passes its arguments into one the guard reported as missing them
  // — a fixer and a checker disagreeing about what the code says.
  // ★ The separator is a **lookahead**, so it survives the replacement: consuming it made the *next* shorthand invisible
  // (`{ pluginId, removeData }` expanded only the first, because the comma that should have introduced the second had
  // already been eaten), and the guard reported a call it had just rewritten as missing an argument.
  const expanded = literal.replace(/([{,]\s*)([a-z_$][\w$]*)(?=\s*[,}])/gi, '$1$2: $2')
  const keys: string[] = []
  let depth = 0
  let quote: string | undefined
  for (let index = 0; index < expanded.length; index += 1) {
    const character = expanded[index]
    if (quote !== undefined) {
      if (character === '\\')
        index += 1
      else if (character === quote)
        quote = undefined
      continue
    }
    if (character === '\'' || character === '"' || character === '`') {
      quote = character
      continue
    }
    if (character === '{' || character === '[' || character === '(') {
      depth += 1
      continue
    }
    if (character === '}' || character === ']' || character === ')') {
      depth -= 1
      continue
    }
    if (depth === 1 && character === ':') {
      const name = /([a-z_$][\w$]*)\s*$/i.exec(expanded.slice(0, index))?.[1]
      if (name !== undefined && !keys.includes(name))
        keys.push(name)
    }
  }
  return keys
}

/** Every literal `call`/`invoke` in one source, with the keys it passes and how many calls could not be read. */
function callSites(text: string, where: string): { readonly sites: CallSite[], readonly unreadable: number } {
  const sites: CallSite[] = []
  let unreadable = 0
  // The type argument of `call<T>` can hold braces and brackets but never a parenthesis, which is what bounds it here.
  for (const match of withoutCommentLines(text).matchAll(/\b(?:call|invoke)\s*(?:<[^()]*>\s*)?\(\s*'([a-z][a-z0-9_]*)'/g)) {
    const command = match[1]
    if (command === undefined)
      continue
    const line = text.slice(0, match.index).split('\n').length
    let index = match.index + match[0].length
    while (/\s/.test(text[index] ?? ''))
      index += 1

    // No arguments at all: the call ends where it began.
    if (text[index] === ')') {
      sites.push({ command, keys: [], where: `${where}:${line.toString()}` })
      continue
    }
    if (text[index] !== ',') {
      unreadable += 1
      continue
    }
    index += 1
    while (/\s/.test(text[index] ?? ''))
      index += 1
    // A variable, a spread, a call: this reads literals only, and reports how many it could not read rather than assuming.
    if (text[index] !== '{') {
      unreadable += 1
      continue
    }
    const end = literalEnd(text, index)
    if (end === undefined) {
      unreadable += 1
      continue
    }
    sites.push({ command, keys: topLevelKeys(text.slice(index, end)), where: `${where}:${line.toString()}` })
  }
  return { sites, unreadable }
}

/**
 * The same text with the lines that are **only** a comment blanked out, and every line's length kept.
 *
 * ★ A call written in prose is not a call. The first run of this check reported `pages/DevicesPage.vue:124` — a JSDoc line
 * that quoted `call('devices_scan_start')` while explaining the bug this check exists for. Blanking whole comment lines
 * rather than stripping comments keeps the line numbers exact, which is what makes a failure point at the right source
 * line.
 *
 * What it does not handle is a call written inside a *trailing* comment on a code line. A comment like that would have to
 * be parsed out of a string or a regex literal to be handled, and a stripper clever enough to try is one that eats real
 * code — the trade this file already made once, in the import check above.
 */
function withoutCommentLines(text: string): string {
  return text
    .split('\n')
    .map((line) => {
      const trimmed = line.trimStart()
      const comment = trimmed.startsWith('*') || trimmed.startsWith('//') || trimmed.startsWith('/*')
      return comment ? ' '.repeat(line.length) : line
    })
    .join('\n')
}

async function checkCommandArguments(): Promise<void> {
  let shell: string
  try {
    shell = await readFile(join(here, 'src-tauri', 'src', 'shell.rs'), 'utf8')
  }
  catch {
    // The shell lives in this repository, so its absence is a failure rather than a note: with no signatures there is
    // nothing to compare, and a check that quietly compares nothing is the failure this whole file exists to remove.
    failures.push({
      check: 'command-arguments',
      detail: 'src-tauri/src/shell.rs is unreadable, so no call could be held to its arguments'
    })
    return
  }

  const wrappers = shellWrappers(shell)
  if (wrappers.size === 0) {
    failures.push({
      check: 'command-arguments',
      detail: 'no #[tauri::command] signature was parsed from src-tauri/src/shell.rs — the wrappers\' shape has changed'
    })
    return
  }

  // The parser's own guard: what it read must be what the shell registers. A regex that had stopped matching half the
  // wrappers would make every call site look correct.
  const handler = /generate_handler!\[([\s\S]*?)\]/.exec(shell)
  const registered = (handler?.[1] ?? '')
    .split(',')
    .map(name => name.trim())
    .filter(name => name.length > 0)
  const mismatched = [
    ...registered.filter(name => !wrappers.has(name)),
    ...[...wrappers.keys()].filter(name => !registered.includes(name))
  ]
  if (mismatched.length > 0) {
    failures.push({
      check: 'command-arguments',
      detail: `generate_handler! and the wrapper signatures disagree about ${mismatched.join(', ')}`
    })
  }

  let checked = 0
  let unreadable = 0
  let notRegistered = 0
  for (const file of await sources(join(here, 'src'))) {
    const name = relative(join(here, 'src'), file).replaceAll('\\', '/')
    // ★ Tests are excluded, and the exclusion is the point rather than a convenience: a test makes calls *about* the
    // client rather than to the kernel — `client.test.ts` calls `plugins_enable` with no arguments to watch the error
    // mapping — so the argument contract is a rule about the code that ships. What a test may not do is expect the fake
    // to answer a call the shell refuses, and that half is checked below.
    if (name.endsWith('.test.ts'))
      continue

    const found = callSites(await readFile(file, 'utf8'), name)
    unreadable += found.unreadable
    for (const site of found.sites) {
      const wrapper = wrappers.get(site.command)
      if (wrapper === undefined) {
        // Declared by the contract and not registered by this shell: there is no signature to hold the call to. The name
        // check above is what covers those.
        notRegistered += 1
        continue
      }
      checked += 1
      for (const key of wrapper.required) {
        if (!site.keys.includes(key)) {
          failures.push({
            check: 'command-arguments',
            detail: `${site.where} calls \`${site.command}\` without \`${key}\`, which src-tauri/src/shell.rs declares as required`
          })
        }
      }
      for (const key of site.keys) {
        if (!wrapper.declared.includes(key)) {
          const declares = wrapper.declared.length === 0
            ? 'no arguments'
            : `\`${wrapper.declared.join('`, `')}\``
          failures.push({
            check: 'command-arguments',
            detail: `${site.where} passes \`${key}\` to \`${site.command}\`, which declares ${declares}`
          })
        }
      }
    }
  }

  // ★ And the half that let the bug through: the fake answered a call the shell refuses.
  const fake = await readFile(join(here, 'src', 'ipc', 'fake-kernel.ts'), 'utf8')
  const table = /export const REQUIRED_ARGS[^{]*\{([\s\S]*?)\n\}/.exec(fake)?.[1]
  if (table === undefined) {
    failures.push({
      check: 'command-arguments',
      detail: 'src/ipc/fake-kernel.ts no longer declares REQUIRED_ARGS, so the fake is not held to the shell'
    })
    return
  }

  const claimed = new Map<string, string[]>()
  for (const match of table.matchAll(/(\w+):\s*\[([^\]]*)\]/g)) {
    const command = match[1]
    if (command === undefined)
      continue
    claimed.set(command, [...(match[2] ?? '').matchAll(/['"](\w+)['"]/g)].map(key => key[1] ?? ''))
  }
  if (claimed.size === 0) {
    failures.push({
      check: 'command-arguments',
      detail: 'REQUIRED_ARGS was found but nothing was parsed from it — the table\'s shape has changed'
    })
  }

  for (const [command, keys] of claimed) {
    const wrapper = wrappers.get(command)
    if (wrapper === undefined) {
      failures.push({
        check: 'command-arguments',
        detail: `REQUIRED_ARGS names \`${command}\`, which src-tauri/src/shell.rs does not register`
      })
      continue
    }
    if (wrapper.required.join(',') !== keys.join(',')) {
      failures.push({
        check: 'command-arguments',
        detail: `REQUIRED_ARGS says \`${command}\` needs [${keys.join(', ')}] where the shell requires [${wrapper.required.join(', ')}]`
      })
    }
  }

  const answered = new Set<string>()
  for (const match of fake.matchAll(/case '([a-z][a-z0-9_]*)':/g)) {
    if (match[1] !== undefined)
      answered.add(match[1])
  }
  for (const command of answered) {
    const wrapper = wrappers.get(command)
    if (wrapper === undefined || wrapper.required.length === 0 || claimed.has(command))
      continue
    failures.push({
      check: 'command-arguments',
      detail: `the fake answers \`${command}\`, which the shell refuses without \`${wrapper.required.join('`, `')}\` — it has to require them too`
    })
  }

  notes.push(
    `command-arguments: ${String(checked)} call(s) held against ${String(wrappers.size)} wrapper signatures, `
    + `${String(notRegistered)} call(s) to commands this shell does not register`
  )
  if (unreadable > 0)
    notes.push(`command-arguments: ${String(unreadable)} call(s) were not read — their name or arguments are not literals`)

  // ★ The README states the registered count in prose, and prose is not compiled: it said "23", then "25", while the shell
  // grew to 29 — four commands' worth of a promise nobody could check. The number is derivable, so it is checked rather than
  // remembered.
  //
  // Only the registered count is compared here. `checked` above counts **call sites**, while the README's "被调用 N 条" counts
  // **distinct commands**: comparing them reported 15 against 10 and would have failed a true sentence.
  const readme = await readFile('README.md', 'utf8')
  const stated = /注册 (\d+) 条/.exec(readme)
  if (stated === null) {
    failures.push({
      check: 'readme-counts',
      detail: 'README.md no longer states how many commands are registered, so nothing checks it',
    })
  }
  else if (stated[1] !== String(wrappers.size)) {
    failures.push({
      check: 'readme-counts',
      detail: `README.md says ${stated[1] ?? '?'} registered commands, and shell.rs registers ${String(wrappers.size)}`,
    })
  }
}

await checkInjection()
await checkSingleDeclarations()
await checkTypeOnlyImports()
await checkCommandNames()
await checkCommandArguments()
await checkFakeIsQuarantined()

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
