import type { PluginCustomEventName } from '@synra/protocol'

type EventMatcher = (name: string) => boolean

const MAX_PATTERN_LENGTH = 240
const MAX_WILDCARDS = 4
const MAX_PLUGIN_ID_LENGTH = 128

function isSlug(part: string): boolean {
  return /^[a-z0-9]/.test(part) && !/[^a-z0-9-]/.test(part)
}

function isPluginId(value: string): boolean {
  if (value.length > MAX_PLUGIN_ID_LENGTH || !value.startsWith('@'))
    return false
  const [scope, name, extra] = value.slice(1).split('/')
  return scope !== undefined && name !== undefined && extra === undefined
    && isSlug(scope) && isSlug(name)
}

export function isCustomEventName(name: string): name is PluginCustomEventName {
  const [prefix, owner, ...topic] = name.split('.')
  return name.length <= MAX_PATTERN_LENGTH && prefix === '_plugin' && owner !== undefined && isPluginId(owner)
    && topic.length > 0 && topic.every(part => part.length > 0 && !/[^\w-]/.test(part))
}

/** Compile patterns before attaching; the native host checks them independently. */
export function compileEventPattern(pattern: unknown): EventMatcher | undefined {
  if (typeof pattern !== 'string' || pattern.length > MAX_PATTERN_LENGTH)
    return undefined
  const parts = pattern.split('.')
  let wildcards = 0
  for (const [index, part] of parts.entries()) {
    if (part === '*') {
      if (++wildcards > MAX_WILDCARDS)
        return undefined
    }
    else if (index === 1 && parts[0] === '_plugin') {
      if (!isPluginId(part))
        return undefined
    }
    else if (part.length === 0 || /[^\w-]/.test(part)) {
      return undefined
    }
  }
  if (wildcards === 0)
    return name => name === pattern
  return (name) => {
    const candidate = name.split('.')
    return parts.length === candidate.length && parts.every((part, index) => {
      const segment = candidate[index]
      return segment !== undefined && segment !== '' && (part === '*' || part === segment)
    })
  }
}
