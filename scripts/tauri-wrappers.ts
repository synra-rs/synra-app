import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { shellWrappers } from './checks/calls'

/** Read the dispatcher and check that each route reaches a command in its wrapper module. */
export function readTauriWrappers(appRoot: string): ReturnType<typeof shellWrappers> {
  const directory = join(appRoot, 'src-tauri', 'src', 'shell', 'wrappers')
  const source = readFileSync(join(directory, 'mod.rs'), 'utf8')
  const modules = [...source.matchAll(/^mod (\w+);$/gm)].flatMap(([, name]) => name ? [name] : [])
  const dispatch = 'match invoke.message.command() {'
  const start = source.indexOf(dispatch)
  const end = source.indexOf('_ => return false', start)
  if (start === -1 || end === -1 || modules.length === 0)
    throw new Error('no Tauri wrapper modules or dispatch routes were found')
  const routes = source.slice(start + dispatch.length, end)

  const commands = new Map<string, string[]>()
  for (const [, names, module] of routes.matchAll(/([^,]+)=>\s*(\w+)::dispatch\s*,/g)) {
    if (!names || !module || !modules.includes(module) || commands.has(module))
      throw new Error(`invalid Tauri dispatch route for ${module}`)
    commands.set(module, [...names.matchAll(/"(\w+)"/g)].flatMap(([, name]) => name ? [name] : []))
  }

  const wrappers: ReturnType<typeof shellWrappers> = new Map()
  for (const module of modules) {
    const path = join(directory, `${module}.rs`)
    const implementation = readFileSync(path, 'utf8')
    const signatures = shellWrappers(implementation)
    const handler = /generate_handler!\[([\s\S]*?)\]/.exec(implementation)?.[1]
    const registered = handler?.split(',').map(name => name.trim()).filter(Boolean) ?? []
    const expected = [...signatures.keys()].sort().join(',')
    if (signatures.size === 0 || registered.sort().join(',') !== expected || commands.get(module)?.sort().join(',') !== expected)
      throw new Error(`${path} has different commands in its dispatch routes, handler and wrapper signatures`)
    for (const [name, signature] of signatures) {
      if (wrappers.has(name))
        throw new Error(`Tauri command ${name} is registered twice`)
      wrappers.set(name, signature)
    }
  }
  return wrappers
}
