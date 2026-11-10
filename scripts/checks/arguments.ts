import { readFile } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { readTauriWrappers } from '../tauri-wrappers'
import { callSites } from './calls'
import { failures, here, notes, sources } from './common'

export async function checkCommandArguments(called: ReadonlySet<string>): Promise<void> {
  let wrappers: ReturnType<typeof readTauriWrappers>
  try {
    wrappers = readTauriWrappers(here)
  }
  catch (error) {
    failures.push({
      check: 'command-arguments',
      detail: error instanceof Error ? error.message : String(error)
    })
    return
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
            detail: `${site.where} calls \`${site.command}\` without its required Tauri argument \`${key}\``
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
        detail: `REQUIRED_ARGS names \`${command}\`, which the Tauri shell does not register`
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
  // Both counts in the development guide stay checked; the README introduces the product.
  //
  // The registered count was guarded after it drifted "23 → 25 while the shell grew to 29". The "called" count was left
  // alone because the call-site total is not the distinct-command total: comparing them reported 15 against 10 and would
  // have failed a true sentence. Now `checkCommandNames` returns the **distinct** set, so the same number can be checked —
  // and it needed to be: the log and export panels added two callers and the prose silently stayed at 29 while the code
  // reached 31.
  const readme = await readFile('CONTRIBUTING.md', 'utf8')
  const stated = /注册 (\d+) 条/.exec(readme)
  if (stated === null) {
    failures.push({
      check: 'readme-counts',
      detail: 'CONTRIBUTING.md no longer states how many commands are registered, so nothing checks it'
    })
  }
  else if (stated[1] !== String(wrappers.size)) {
    failures.push({
      check: 'readme-counts',
      detail: `CONTRIBUTING.md says ${stated[1] ?? '?'} registered commands, and shell.rs registers ${String(wrappers.size)}`
    })
  }
  const calledStated = /被调用 (\d+) 条/.exec(readme)
  if (calledStated === null) {
    failures.push({
      check: 'readme-counts',
      detail: 'CONTRIBUTING.md no longer states how many commands are called, so nothing checks it'
    })
  }
  else if (calledStated[1] !== String(called.size)) {
    failures.push({
      check: 'readme-counts',
      detail: `CONTRIBUTING.md says ${calledStated[1] ?? '?'} called commands, and src calls ${String(called.size)} distinct ones`
    })
  }
}
