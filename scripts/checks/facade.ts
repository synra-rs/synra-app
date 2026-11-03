import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { build } from 'vite'
import { failures, here, notes } from './common'

export async function checkFacadeIsFresh(): Promise<void> {
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
