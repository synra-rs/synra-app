import type { IpcError, RegistryProblem } from '@synra/protocol'

interface Source { readonly kind: 'npm' | 'static', readonly url: string }
type Parsed = { readonly ok: true, readonly sources: readonly Source[] } | { readonly ok: false, readonly error: IpcError }

function refusal(problem: RegistryProblem): IpcError {
  const code = problem.kind === 'invalidConfiguration' ? 'invalid_argument' : 'unsupported'
  return { code, message: `error.${code}`, retryable: false, details: { registry: problem }, actions: [{ kind: 'openSettings', section: 'plugins' }] }
}

/** Browser-demo validation. Rust-produced settings vectors check its decisions against the native parser. */
export function previewRegistry(value: unknown): Parsed {
  const invalid = (reason: string, index?: number): Parsed => ({
    ok: false,
    error: refusal({ kind: 'invalidConfiguration', field: 'plugins.registry', reason, ...(index === undefined ? {} : { index }) })
  })
  if (value === undefined || (typeof value === 'string' && !value.trim()))
    return { ok: true, sources: [] }
  if (typeof value !== 'string' && !Array.isArray(value))
    return invalid('Choose a URL string or an array of URL strings.')
  const items = typeof value === 'string' ? [value] : value
  const sources: Source[] = []
  for (let at = 0; at < items.length; at++) {
    const item: unknown = items[at]
    const index = Array.isArray(value) ? at : undefined
    if (typeof item !== 'string' || !item.trim())
      return invalid('Every mirror must be a nonempty URL string.', index)
    const text = item.trim()
    const kind = text.startsWith('static:') ? 'static' : 'npm'
    const address = kind === 'static' ? text.slice('static:'.length).trim() : text
    try {
      const authority = address.split('://')[1]
      if (!authority || /^[/?#]/.test(authority) || /[\s\\]/.test(address))
        return invalid('Enter an absolute registry URL.', index)
      const url = new URL(address)
      if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.search || url.hash
        || address.includes('?') || address.includes('#') || (kind === 'npm' && url.pathname.replaceAll('/', '') !== '')) {
        return invalid('Enter a registry base without credentials or a query.', index)
      }
      if (url.protocol === 'http:' && url.hostname !== 'localhost' && url.hostname !== '[::1]' && !/^127\./.test(url.hostname))
        return invalid('HTTP is permitted only for a loopback registry.', index)
      sources.push({ kind, url: url.href.replace(/\/+$/, '') })
    }
    catch {
      return invalid('Enter a valid registry URL.', index)
    }
  }
  return { ok: true, sources }
}

export function previewRegistrySearch(value: unknown): IpcError | undefined {
  const parsed = previewRegistry(value)
  if (!parsed.ok)
    return parsed.error
  if (!parsed.sources.length)
    return refusal({ kind: 'notConfigured' })
  if (!parsed.sources.some(source => source.kind === 'npm'))
    return refusal({ kind: 'searchUnsupported' })
  return undefined
}
