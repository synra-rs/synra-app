import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { extname, join } from 'node:path'
import process from 'node:process'

const limit = 300
const extensions = new Set([
  '.rs',
  '.ts',
  '.tsx',
  '.mts',
  '.cts',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.vue',
  '.html',
  '.css',
  '.scss',
  '.sass',
  '.less'
])
const generated = new Set([
  'src-tauri/inject/facade.js',
  'src/auto-generated/build-stamp.ts',
  'src/auto-generated/core-build-stamp.d.ts'
])

function physicalLines(source: string): number {
  const breaks = source.match(/\r\n|\r|\n/g)?.length ?? 0
  return breaks + (source && !/[\r\n]$/.test(source) ? 1 : 0)
}

function checkSourceSize(): void {
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], {
    encoding: 'utf8'
  }).replace(/\r?\n$/, '')
  const listing = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
    cwd: root,
    encoding: 'utf8'
  })
  let checked = 0
  for (const path of new Set(listing.split('\0'))) {
    if (generated.has(path) || !extensions.has(extname(path).toLowerCase()))
      continue

    let source: string
    try {
      source = readFileSync(join(root, path), 'utf8')
    }
    catch (error) {
      // A tracked file can be deleted from the worktree before staging.
      if ((error as NodeJS.ErrnoException).code === 'ENOENT')
        continue
      throw error
    }

    checked += 1
    const count = physicalLines(source)
    if (count > limit) {
      console.error(`${path}: ${count} lines (limit ${limit})`)
      process.exitCode = 1
    }
  }

  if (!process.exitCode)
    console.log(`Checked ${checked} source files (limit ${limit} lines)`)
}

checkSourceSize()
