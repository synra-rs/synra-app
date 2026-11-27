/** Core DSL parsing and matching. Rust producer vectors calibrate this local precheck. */
const DOMAINS = new Set(['log', 'event', 'device', 'storage', 'network', 'fs', 'file', 'platform', 'action', 'ui'])

function parseCapability(text: string): readonly string[] | undefined {
  // Only ASCII is accepted, so a valid input's character count is its byte count.
  if (text.length > 256)
    return undefined
  const parts = text.split(':')
  if (parts.length > 6 || !DOMAINS.has(parts[0] ?? '') || parts.some(part => part.length === 0))
    return undefined
  for (let index = 1; index < parts.length; index++) {
    const part = parts[index]
    if (part === undefined)
      return undefined
    if (part === '*' || part === '**')
      continue
    if (index === 1 ? !/^[a-z]/.test(part) || part.length > 64 || /[^a-z0-9-]/.test(part) : /[^\w.@/*-]/.test(part))
      return undefined
  }
  return parts
}

function segmentMatches(pattern: string, candidate: string): boolean {
  if (pattern === candidate || pattern === '*')
    return true
  if (pattern.endsWith('**'))
    return candidate.startsWith(pattern.slice(0, -2))
  if (pattern.endsWith('*')) {
    const stem = pattern.slice(0, -1)
    return candidate.startsWith(stem) && !/[/.]/.test(candidate.slice(stem.length))
  }
  return false
}

function matches(pattern: readonly string[], candidate: readonly string[]): boolean {
  if (pattern[0] !== candidate[0])
    return false
  let p = 1
  let c = 1
  let star: { pattern: number, consumed: number } | undefined
  while (c < candidate.length) {
    const segment = pattern[p]
    const target = candidate[c]
    if (segment === '**') {
      star = { pattern: p, consumed: c }
      p++
    }
    else if (segment !== undefined && target !== undefined && segmentMatches(segment, target)) {
      p++
      c++
    }
    else if (star !== undefined) {
      p = star.pattern + 1
      c = ++star.consumed
    }
    else {
      return false
    }
  }
  return pattern.slice(p).every(part => part === '**')
}

/** Synchronous query of the page's injected snapshot. Rust still checks current grants for each RPC. */
export type GrantCheck = (capability: string) => boolean

export function compileGrantCheck(grants: readonly string[]): GrantCheck {
  const patterns: (readonly string[])[] = []
  for (const grant of grants) {
    const pattern = parseCapability(grant)
    if (pattern === undefined)
      return () => false
    patterns.push(pattern)
  }
  return (capability) => {
    const wanted = parseCapability(capability)
    return wanted !== undefined && patterns.some(pattern => matches(pattern, wanted))
  }
}
