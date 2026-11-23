import { data } from './shape'

const SECRET_FIELD = /token|secret|password|passwd|credential|api[_-]?key|auth|cookie|private[_-]?key|signature|payload|clipboard|file_?data|keyring|^body$|^text$|^content$|^bytes$|^request$/i
const MAX_STRING = 512
const MAX_CHARACTERS = 4096
const MAX_NODES = 96
const MAX_DEPTH = 4

export function diagnosticText(value: string): string {
  return value.slice(0, MAX_STRING)
    .replace(/https?:\/\/[^\s"'<>]+/gi, (text) => {
      try {
        const url = new URL(text)
        url.username = ''
        url.password = ''
        url.search = ''
        url.hash = ''
        return url.href
      }
      catch {
        return '[invalid URL]'
      }
    })
    .replace(/\b(authorization|bearer|token|password|secret|api[_-]?key)(\s*[:=]\s*|\s+)[^\s,;]+/gi, '$1=[REDACTED]')
}

/** Bounded diagnostic data, without stringifying an arbitrary bridge rejection. */
export function diagnostic(value: unknown): unknown {
  const seen = new WeakSet<object>()
  let nodes = MAX_NODES
  let characters = MAX_CHARACTERS
  const visit = (input: unknown, depth: number): unknown => {
    if (--nodes < 0 || characters <= 0 || depth > MAX_DEPTH)
      return '[truncated]'
    if (typeof input === 'string') {
      const text = diagnosticText(input).slice(0, characters)
      characters -= text.length
      return text
    }
    if (input === null || typeof input === 'boolean')
      return input
    if (typeof input === 'number')
      return Number.isFinite(input) ? input : null
    if (typeof input !== 'object')
      return '[unavailable]'
    if (seen.has(input))
      return '[circular]'
    seen.add(input)
    try {
      if (Array.isArray(input)) {
        const length = data(input, 'length')
        if (typeof length !== 'number')
          return '[unavailable]'
        const output: unknown[] = []
        for (let index = 0; index < length; index++) {
          if (nodes <= 0 || characters <= 0)
            break
          output.push(visit(data(input, String(index)), depth + 1))
        }
        return output
      }
      const output: Record<string, unknown> = {}
      for (const key of Object.keys(input).slice(0, Math.max(0, nodes))) {
        if (characters <= 0 || nodes <= 0)
          break
        const name = key.slice(0, 64)
        characters -= name.length
        Object.defineProperty(output, name, {
          value: SECRET_FIELD.test(key) ? '[REDACTED]' : visit(data(input, key), depth + 1),
          enumerable: true
        })
      }
      return output
    }
    catch {
      return '[unavailable]'
    }
  }
  return visit(value, 0)
}
