/**
 * The log namespace: the one API that must always work.
 *
 * # Why this writes to the page's console and not to the kernel
 *
 * The message reaches the developer looking at the plugin's own devtools, which is where they already are. The kernel
 * has no log ring yet, and routing through a command that does not exist would make every `log.info` a rejected
 * promise — which is worse than a missing feature, because it breaks the one API that must always work.
 */

import type { LogNamespace } from '@synra/client'

/** The log namespace, writing to the console the plugin's author is already looking at. */
export function createLogNamespace(): LogNamespace {
  const log: LogNamespace = {
    // The `no-console` allowance in this repository is `warn` and `error`, and it is deliberately overridden here for
    // `trace`/`debug`/`info`. The rule exists to keep stray prints out of the shell's own code — a debug statement
    // nobody meant to leave — and this block is not one: it is the storage a plugin's log namespace is *specified* to
    // write into. Mapping the three lower levels onto `warn` would be worse than the exception, because a plugin author
    // filtering by level would then see its `info` lines filed under warnings.
    /* eslint-disable no-console */
    trace: (message: string, context_?: Record<string, unknown>) => {
      console.trace(message, context_)
    },
    debug: (message: string, context_?: Record<string, unknown>) => {
      console.debug(message, context_)
    },
    info: (message: string, context_?: Record<string, unknown>) => {
      console.info(message, context_)
    },
    /* eslint-enable no-console */
    warn: (message: string, context_?: Record<string, unknown>) => {
      console.warn(message, context_)
    },
    error: (message: string, context_?: Record<string, unknown>) => {
      console.error(message, context_)
    },
    // ★ A prefix on every line, and a real object rather than a spread.
    //
    // The prefix is what makes a group of messages readable in a console full of other plugins' output, and the sublogger
    // nests because `child('net').child('fetch')` should read `net.fetch` — not because nesting was required, but
    // because a child logger that loses its parent's prefix is a bug waiting to be found in someone else's log.
    child: (prefix: string): LogNamespace => {
      const tagged = (method: 'trace' | 'debug' | 'info' | 'warn' | 'error') =>
        (message: string, fields?: Record<string, unknown>) => {
          log[method](`${prefix} ${message}`, fields)
        }
      return {
        trace: tagged('trace'),
        debug: tagged('debug'),
        info: tagged('info'),
        warn: tagged('warn'),
        error: tagged('error'),
        child: (nested: string) => log.child(`${prefix}.${nested}`)
      }
    }
  }
  return log
}
