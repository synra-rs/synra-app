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

import { mkdtemp, readdir, readFile, rm, stat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, relative } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import { build } from 'vite'

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
 *
 * Returns the **distinct** set of called names, which `readme-counts` compares with the README's "被调用 N 条" — that
 * number was prose nobody checked until it drifted by two.
 */
async function checkCommandNames(): Promise<ReadonlySet<string>> {
  // Every string this repository passes as a command. The names appear as the first argument of `call` and in the
  // method table below, and both are `'name'` literals.
  //
  // ★ The receiver is part of the pattern, and `self.invoke` is why. The first version matched any `.call(`/`.invoke(`
  // with a lowercase literal and read `synra.self.invoke('clear')` — a **plugin action**, not a command — as a call to a
  // command the kernel does not declare. Naming the seam (`kernel`, `client`, the injected `bridge`) is the rule the
  // check actually means: those are the only objects whose call names are commands. The type argument is allowed because
  // `kernel.call<DevicesSnapshot>('devices_snapshot')` is the ordinary form.
  //
  // ★ Read **before** the kernel checkout, because the returned set also feeds the README's "called" count, and a missing
  // sibling repository must not silently disable that check.
  const called = new Set<string>()
  for (const file of await sources(join(here, 'src'))) {
    const text = await readFile(file, 'utf8')
    for (const match of text.matchAll(/\b(?:kernel|client|bridge)\.(?:call|invoke)\s*(?:<[^()]*>\s*)?\(\s*'([a-z][a-z0-9_]*)'/g)) {
      if (match[1] !== undefined)
        called.add(match[1])
    }
  }

  const rust = join(here, '..', 'synra-rust-core', 'crates', 'synra-ipc', 'src', 'command.rs')
  let rustText: string
  try {
    rustText = await readFile(rust, 'utf8')
  }
  catch {
    notes.push('command-names: the kernel checkout is not beside this one, so the names were not compared')
    return called
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
    return called
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
  return called
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
      // ★ `app: tauri::AppHandle` is resolved by `#[tauri::command]` from the invocation context, exactly like `state` —
      // not deserialized from the message. This was found by `plugin_ui_open`'s first caller: the page sends only
      // `pluginId`, the guard reported `app` as a missing required key, and the command was right.
      if (type.startsWith('tauri::AppHandle'))
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

async function checkCommandArguments(called: ReadonlySet<string>): Promise<void> {
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

  // ★ Both counts the README states are checked, and the second used to be exempt.
  //
  // The registered count was guarded after it drifted "23 → 25 while the shell grew to 29". The "called" count was left
  // alone because the call-site total is not the distinct-command total: comparing them reported 15 against 10 and would
  // have failed a true sentence. Now `checkCommandNames` returns the **distinct** set, so the same number can be checked —
  // and it needed to be: the log and export panels added two callers and the prose silently stayed at 29 while the code
  // reached 31.
  const readme = await readFile('README.md', 'utf8')
  const stated = /注册 (\d+) 条/.exec(readme)
  if (stated === null) {
    failures.push({
      check: 'readme-counts',
      detail: 'README.md no longer states how many commands are registered, so nothing checks it'
    })
  }
  else if (stated[1] !== String(wrappers.size)) {
    failures.push({
      check: 'readme-counts',
      detail: `README.md says ${stated[1] ?? '?'} registered commands, and shell.rs registers ${String(wrappers.size)}`
    })
  }
  const calledStated = /被调用 (\d+) 条/.exec(readme)
  if (calledStated === null) {
    failures.push({
      check: 'readme-counts',
      detail: 'README.md no longer states how many commands are called, so nothing checks it'
    })
  }
  else if (calledStated[1] !== String(called.size)) {
    failures.push({
      check: 'readme-counts',
      detail: `README.md says ${calledStated[1] ?? '?'} called commands, and src calls ${String(called.size)} distinct ones`
    })
  }
}

/**
 * 7. The committed façade is what its source builds.
 *
 * ★ Why this check exists.
 *
 * `src-tauri/inject/facade.js` is generated from `src/ipc/injection/bootstrap.ts` and then **embedded in the shell with
 * `include_str!`** — so a stale copy is not a cosmetic problem: it is the code a plugin page actually runs, and the
 * source it was built from would be fiction. The file is committed because a Rust build must not need Node; this check is
 * what makes the commit honest.
 *
 * The comparison is on the rebuilt bytes, not on a hash a script could forget to update: a build is the only thing that
 * proves the two agree.
 */
async function checkFacadeIsFresh(): Promise<void> {
  const committed = join(here, 'src-tauri', 'inject', 'facade.js')
  const directory = await mkdtemp(join(tmpdir(), 'synra-facade-'))
  const normalise = (text: string): string => text.replaceAll('\r\n', '\n')

  try {
    await build({
      root: here,
      configFile: join(here, 'vite.inject.config.ts'),
      // The output goes somewhere disposable: on a good day the build proves the committed file, and on a bad day it
      // proves nothing but must not have rewritten the file it is checking.
      build: { outDir: directory, emptyOutDir: true },
      logLevel: 'silent'
    })
    const rebuilt = normalise(await readFile(join(directory, 'facade.js'), 'utf8'))
    const current = normalise(await readFile(committed, 'utf8'))
    if (rebuilt !== current) {
      failures.push({
        check: 'facade-fresh',
        detail: 'src-tauri/inject/facade.js does not match its source — run `pnpm run build:facade`'
      })
    }
    else {
      notes.push('facade: the committed façade is what `src/ipc/injection/bootstrap.ts` builds')
    }
  }
  catch (cause) {
    failures.push({ check: 'facade-fresh', detail: `the façade could not be rebuilt: ${String(cause)}` })
  }
  finally {
    await rm(directory, { recursive: true, force: true })
  }
}

/**
 * Every file that draws an `AButton` carries the touch floor itself.
 *
 * # Why this is a script and not a habit
 *
 * `AButton` is always `size="small"` — 24px. The floor comes from one utility on an ancestor, and until 16.97 three of
 * the four pages simply did not have it: their body buttons were 24px while the devices page's were 44. Nothing caught
 * it, because no test reads computed style, and a missing parent utility looks exactly like a page that has none.
 *
 * # Why it now reads every file, not just the pages
 *
 * The first version checked only `src/pages`, on the reasoning that components are nested inside a page that already
 * raises everything. `ErrorNotice` is the counter-example that reasoning missed: it is drawn inside an `AAlert`, which
 * sits in `SettingsFeedback` — neither carries the floor — and it is used from twenty places, most of them outside
 * settings. Its two buttons were 24px, and this script reported nothing because it never opened them (16.151).
 *
 * The rule is therefore on the drawing file: **whichever file renders an `AButton` must be able to show the floor**,
 * either as the utility on an ancestor in that same file, or as `min-height: 44px` in its own scoped CSS. A component
 * that merely *relies* on an ancestor elsewhere cannot satisfy this, and that is the point — a dependency nobody wrote
 * down is the one that goes missing.
 */
async function checkPageButtons(): Promise<void> {
  const floor = '[&_.ant-btn]:min-h-11'
  // `PairingPanel` and `TransferPrompt` state the same 44px in scoped CSS, because their button colour lives there and
  // a utility cannot reach into `:deep()`. Both forms are the floor; neither is "the right one".
  const scopedFloor = /min-height:\s*44px/
  const files = await sources(join(here, 'src'))
  let counted = 0

  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    // Comments mention `<AButton` while explaining it; strip them before deciding what the file draws.
    const text = (await readFile(file, 'utf8')).replace(/<!--[\s\S]*?-->/g, match => '\n'.repeat(match.split('\n').length))
    if (!text.includes('<AButton'))
      continue
    counted += 1
    if (!text.includes(floor) && !scopedFloor.test(text)) {
      failures.push({
        check: 'page-buttons',
        detail: `${relative(here, file)} draws buttons but shows no touch floor (neither \`${floor}\` nor \`min-height: 44px\`)`
      })
    }
  }

  notes.push(`page-buttons: ${counted.toString()} file(s) raise their own body buttons`)
}

/**
 * Every dialog has to be named, or a screen reader announces an unnamed "dialog".
 *
 * `vc-dialog`'s `Dialog.js` sets exactly one name source on the `role="dialog"` element —
 * `"aria-labelledby": title ? ariaIdRef.value : null` — so `:title` names it. A dialog that deliberately has no title
 * bar names itself through `:wrap-props`, which is spread onto that same element after `role`.
 *
 * ★ This was a real defect once: `SearchPalette` has no title (a bar above a search field is a line between the user
 *   and the box) and no `wrap-props` either, so it announced itself as "dialog" and nothing else.
 *
 * ★ Read the whole opening tag, not a fixed number of lines. The first draft of this check looked four lines past
 *   `<AModal` and reported four false failures — the attributes run six or more lines on most of them.
 */
async function checkDialogNames(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0

  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    let index = text.indexOf('<AModal')
    while (index !== -1) {
      const close = text.indexOf('>', index)
      const tag = text.slice(index, close === -1 ? undefined : close)
      counted += 1
      if (!/:title=|:wrap-props=/.test(tag)) {
        const line = text.slice(0, index).split('\n').length
        failures.push({
          check: 'dialog-names',
          detail: `${relative(here, file)}:${line.toString()} opens a dialog with neither \`:title\` nor \`:wrap-props=\``
        })
      }
      index = text.indexOf('<AModal', index + 1)
    }
  }

  notes.push(`dialog-names: ${counted.toString()} dialog(s) are named`)
}

/**
 * Every `ASelect` and every `AInputNumber` is named.
 *
 * ★ Neither renders a name on its own. `ASelect` draws a combobox and `AInputNumber` a spin button, and where the
 *   name has to go depends on how the template gives it: `aria-label` and `:id` both work for a bare control, while
 *   one inside a `<label>` is named by containment and needs neither.
 *
 * ★ Two real defects both had every way missing: `plugins.catalog.sort` (an unnamed combobox between the search box
 *   and the button) and the diagnostics export's line count (an unnamed spin button beside the words "Log lines" —
 *   visually paired, two unrelated nodes to a screen reader).
 *
 *   The check accepts any of the ways rather than demanding one: the device page's step select is inside a `<label>`
 *   and is correctly named without an attribute, and asking it for one would be a false failure.
 */
async function checkControlNames(): Promise<void> {
  const files = await sources(join(here, 'src'))
  const controls = [
    { tag: 'ASelect', pattern: /<ASelect\b[^>]*>/g },
    { tag: 'AInputNumber', pattern: /<AInputNumber\b[^>]*>/g }
  ] as const
  const counts: string[] = []

  for (const { tag, pattern } of controls) {
    let counted = 0
    for (const file of files) {
      if (!file.endsWith('.vue'))
        continue
      const text = await readFile(file, 'utf8')
      for (const match of text.matchAll(pattern)) {
        counted += 1
        if (!/aria-label|:id=|id=|:aria-labelledby/.test(match[0])) {
          const line = text.slice(0, match.index).split('\n').length
          failures.push({
            check: 'control-names',
            detail: `${relative(here, file)}:${line.toString()} draws an \`${tag}\` with no \`aria-label\`, no \`id\` and no enclosing label`
          })
        }
      }
    }
    counts.push(`${counted.toString()} ${tag}`)
  }

  notes.push(`control-names: ${counts.join(', ')} are named`)
}

/**
 * The two places that raise controls with a scoped media query agree on where "narrow" ends.
 *
 * ★ This was a real inconsistency: `PairingPanel` used `max-width: 767px` and `TransferPrompt` `max-width: 639px`.
 *   `03 §3`'s table puts the narrow layout at `<640`, and the utility-class route (`min-h-11 md:min-h-8`) drops back
 *   to 32px at `md` — so between 640 and 767 the pairing panel alone kept 44px buttons while every other surface on
 *   the same screen had 32px ones.
 *
 *   The utility-class sites are not checked here: their breakpoint lives in the class name, which is one shape.
 */
async function checkTouchBreakpoints(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0

  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    // ★ Read the media query by its two conditions, not by the order they are written in. The first version matched
    //   the literal `(max-width: …), (pointer: coarse)`, which silently skipped `TransferPrompt` — it writes
    //   `(pointer: coarse), (max-width: 639px)`. The check reported "the one scoped touch query", and there were two
    //   (16.153). Both orders are the same query; a guard that only understands one of them stops being a guard.
    for (const match of text.matchAll(/@media[^{}]*(?:max-width:\s*(\d+)px|pointer:\s*coarse)[^{}]*\{/g)) {
      const query = match[0]
      if (!query.includes('pointer: coarse') || !query.includes('max-width'))
        continue
      counted += 1
      const width = /max-width:\s*(\d+)px/.exec(query)?.[1]
      if (width !== '639') {
        const line = text.slice(0, match.index).split('\n').length
        failures.push({
          check: 'touch-breakpoints',
          detail: `${relative(here, file)}:${line.toString()} raises touch targets from ${width ?? '?'}px — every scoped touch query ends at 639px`
        })
      }
    }
  }

  notes.push(
    counted === 1
      ? 'touch-breakpoints: the one scoped touch query ends at 639px'
      : `touch-breakpoints: ${counted.toString()} scoped touch queries end at 639px`
  )
}

/**
 * Corners come from the ladder — `rounded-control` (8), `rounded-surface` (12), `rounded-pill` — never bare `rounded`.
 *
 * ★ `rounded` is 4px: a value the ladder does not contain. Three real cases had drifted onto it — the pairing panel
 *   and the transfer prompt (both surfaces, both should have been 12) and the search entry's `kbd` next to its own
 *   8px field, where the keycap was visibly squarer than the box around it.
 *
 * ★ One legitimate use is not a radius at all: a bare text button (`border-0`, `bg-transparent`, `px-0`) has nothing
 *   for a corner to clip, so the class is dead weight there and was removed rather than given a value. A scan cannot
 *   tell the two apart, so the check names the file and line and lets the author decide which it is.
 */
async function checkCornerLadder(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0

  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    // ★ Comments are stripped first: this rule is about the class the browser gets, and a note that says "not bare
    //   `rounded`" is the opposite of a violation. The strip keeps newlines so positions still map to the file.
    const code = text
      .replace(/<!--[\s\S]*?-->/g, comment => comment.replace(/[^\n]/g, ' '))
      .replace(/\/\*[\s\S]*?\*\//g, comment => comment.replace(/[^\n]/g, ' '))
      .replace(/\/\/[^\n]*/g, comment => comment.replace(/[^\n]/g, ' '))
    for (const match of code.matchAll(/\brounded(?![-:\w])|\brounded-(?:sm|md|lg|xl|2xl|3xl|none)\b/g)) {
      counted += 1
      const line = code.slice(0, match.index).split('\n').length
      failures.push({
        check: 'corner-ladder',
        detail: `${relative(here, file)}:${line.toString()} uses \`${match[0]}\`, which is not on the ladder — use rounded-control, rounded-surface or rounded-pill`
      })
    }
  }

  notes.push(
    counted === 0
      ? 'corner-ladder: every corner comes from the ladder'
      : `corner-ladder: ${counted.toString()} corner(s) off the ladder`
  )
}

/**
 * A native `<button>` clears the user agent's own look.
 *
 * ★ This is the defect that took four attempts to find. The user agent sheet gives every `<button>`
 *   `background-color: buttonface` and `1px solid buttonborder`, so a class list that says nothing about background
 *   still gets a grey tile with a frame — and no utility class can win until those are reset. The search entry looked
 *   "embossed" across three restylings for exactly this reason, and every option in the search results was a grey
 *   block because the same reset was missing there.
 *
 * ★ `border-0` + `bg-transparent` are the pair. One without the other leaves half the default look, which is how a
 *   ghost button ends up with a frame and no fill. Comments are stripped first: several of these buttons now carry a
 *   note that names both classes, and a note is not the class.
 */
async function checkNativeButtons(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0

  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    const code = text.replace(/<!--[\s\S]*?-->/g, comment => comment.replace(/[^\n]/g, ' '))
    for (const match of code.matchAll(/<button\b[\s\S]{0,800}?>/g)) {
      counted += 1
      const noBorder = /border-0|border-none/.test(match[0])
      const noFill = /bg-transparent/.test(match[0])
      if (!noBorder || !noFill) {
        const line = code.slice(0, match.index).split('\n').length
        const missing = [!noBorder && '`border-0`', !noFill && '`bg-transparent`'].filter(Boolean).join(' and ')
        failures.push({
          check: 'native-buttons',
          detail: `${relative(here, file)}:${line.toString()} draws a native \`<button>\` without ${missing} — the user agent's tile and frame show through`
        })
      }
    }
  }

  notes.push(`native-buttons: ${counted.toString()} native \`<button>\`${counted === 1 ? '' : 's'} clear the user agent look`)
}

await checkInjection()
await checkSingleDeclarations()
await checkTypeOnlyImports()
const called = await checkCommandNames()
await checkCommandArguments(called)
await checkFakeIsQuarantined()
await checkFacadeIsFresh()
await checkPageButtons()
await checkDialogNames()
await checkControlNames()
await checkTouchBreakpoints()
await checkCornerLadder()
await checkNativeButtons()

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
