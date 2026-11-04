import { readFile, stat } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { readIpcContract } from '../ipc-contract'
import { clientTypes, failures, here, notes, sources } from './common'

export async function checkInjection(): Promise<void> {
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

export async function checkSingleDeclarations(): Promise<void> {
  const names = ['SynraResponse', 'SynraError'] as const
  const own = await sources(join(here, 'src'))
  for (const name of names) {
    for (const file of own) {
      const text = await readFile(file, 'utf8')

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

export async function checkTypeOnlyImports(): Promise<void> {
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

export async function checkCommandNames(): Promise<ReadonlySet<string>> {
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
  let rust: string
  let rustText: string
  try {
    const contract = readIpcContract(here)
    rust = contract.path
    rustText = contract.source
  }
  catch (cause) {
    failures.push({ check: 'command-names', detail: String(cause) })
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

export async function checkFakeIsQuarantined(): Promise<void> {
  const root = join(here, 'src')
  const allowed = new Set(['kernel.ts'])
  let importers = 0
  // ★ The rule is about the *kind* of file, not the one name that happened to break it first. `fake-kernel` was the
  //   import that reached production, but the same mistake is available from any fixture: `client-test-helpers`,
  //   `dispatcher-test-helpers`, `injection-test-helpers`, `fake-kernel-fixtures`. None of them is named here yet —
  //   that is the point of checking the pattern rather than the incident.
  //
  //   `fake-kernel.ts` imports `fake-kernel-fixtures`, and it is the file that *is* the fake; it is exempt below for
  //   the same reason `kernel.ts` is allowed to know the double's name.
  const fixture = /test-helpers|fake-kernel|fixtures|(?:^|\/)preview-(?:devices|plugins)(?:$|\.)/
  for (const file of await sources(root)) {
    const name = relative(join(here, 'src'), file).replaceAll('\\', '/')
    // Tests are the place a fake belongs, so they are not the thing being guarded against.
    if (name.endsWith('.test.ts') || name.endsWith('.testkit.ts') || allowed.has(name))
      continue
    // The double itself composes its own fixtures; that is inside the quarantine, not through it.
    const isTheDouble = ['ipc/fake-kernel.ts', 'ipc/preview-devices.ts', 'ipc/preview-plugins.ts'].includes(name)
    const text = await readFile(file, 'utf8')
    for (const match of text.matchAll(/(?:from|import)\s*\(?['"]([^'"]+)['"]/g)) {
      const specifier = match[1]!
      if (!fixture.test(specifier))
        continue
      if (isTheDouble && /fixtures|preview-(?:devices|plugins)/.test(specifier))
        continue
      importers += 1
      failures.push({
        check: 'fake-quarantine',
        detail: `${name} imports \`${specifier}\`; only kernel.ts may choose the fake`
      })
    }
  }
  notes.push(
    importers === 0
      ? 'fake-quarantine: only kernel.ts names the fake kernel'
      : `fake-quarantine: ${String(importers)} production file(s) import the fake`
  )
}
