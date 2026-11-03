export interface CallSite {
  readonly command: string
  readonly keys: readonly string[]
  /** `pages/DevicesPage.vue:265`, so a failure names a line rather than a file. */
  readonly where: string
}

export interface Wrapper {
  readonly declared: readonly string[]
  readonly required: readonly string[]
}

export function camelCase(name: string): string {
  return name.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
}

export function splitParameters(text: string): string[] {
  const parts: string[] = []
  let depth = 0
  let current = ''
  for (const character of text) {
    if (character === '<' || character === '(' || character === '[')
      depth += 1
    else if (character === '>' || character === ')' || character === ']')
      depth -= 1
    if (character === ',' && depth === 0) {
      parts.push(current)
      current = ''
      continue
    }
    current += character
  }
  parts.push(current)
  return parts.map(part => part.trim()).filter(part => part.length > 0)
}

export function shellWrappers(source: string): Map<string, Wrapper> {
  const wrappers = new Map<string, Wrapper>()

  for (const match of source.matchAll(/#\[tauri::command(?:\([^)]*\))?\]\s*pub (?:async )?fn\s+(\w+)\s*\(([\s\S]*?)\)\s*->/g)) {
    const name = match[1]
    const parameters = match[2]
    if (name === undefined || parameters === undefined)
      continue
    const declared: string[] = []
    const required: string[] = []
    for (const parameter of splitParameters(parameters)) {
      const colon = parameter.indexOf(':')
      const field = colon === -1 ? '' : parameter.slice(0, colon).trim()
      const type = colon === -1 ? '' : parameter.slice(colon + 1).trim()
      if (!/^\w+$/.test(field) || type.length === 0)
        continue

      if (field === 'state' || type.startsWith('tauri::State'))
        continue

      if (type.startsWith('tauri::AppHandle'))
        continue
      const key = camelCase(field)
      declared.push(key)

      if (!type.startsWith('Option<'))
        required.push(key)
    }
    wrappers.set(name, { declared, required })
  }
  return wrappers
}

export function literalEnd(text: string, start: number): number | undefined {
  let depth = 0
  let quote: string | undefined
  for (let index = start; index < text.length; index += 1) {
    const character = text[index]
    if (quote !== undefined) {
      if (character === '\\')
        index += 1
      else if (character === quote)
        quote = undefined
      continue
    }
    if (character === '\'' || character === '"' || character === '`') {
      quote = character
      continue
    }
    if (character === '{' || character === '[' || character === '(') {
      depth += 1
    }
    else if (character === '}' || character === ']' || character === ')') {
      depth -= 1
      if (depth === 0)
        return index + 1
    }
  }
  return undefined
}

export function topLevelKeys(literal: string): string[] {
  const expanded = literal.replace(/([{,]\s*)([a-z_$][\w$]*)(?=\s*[,}])/gi, '$1$2: $2')
  const keys: string[] = []
  let depth = 0
  let quote: string | undefined
  for (let index = 0; index < expanded.length; index += 1) {
    const character = expanded[index]
    if (quote !== undefined) {
      if (character === '\\')
        index += 1
      else if (character === quote)
        quote = undefined
      continue
    }
    if (character === '\'' || character === '"' || character === '`') {
      quote = character
      continue
    }
    if (character === '{' || character === '[' || character === '(') {
      depth += 1
      continue
    }
    if (character === '}' || character === ']' || character === ')') {
      depth -= 1
      continue
    }
    if (depth === 1 && character === ':') {
      const name = /([a-z_$][\w$]*)\s*$/i.exec(expanded.slice(0, index))?.[1]
      if (name !== undefined && !keys.includes(name))
        keys.push(name)
    }
  }
  return keys
}

export function callSites(text: string, where: string): { readonly sites: CallSite[], readonly unreadable: number } {
  const sites: CallSite[] = []
  let unreadable = 0
  for (const match of withoutCommentLines(text).matchAll(/\b(?:call|invoke)\s*(?:<[^()]*>\s*)?\(\s*'([a-z][a-z0-9_]*)'/g)) {
    const command = match[1]
    if (command === undefined)
      continue
    const line = text.slice(0, match.index).split('\n').length
    let index = match.index + match[0].length
    while (/\s/.test(text[index] ?? ''))
      index += 1

    if (text[index] === ')') {
      sites.push({ command, keys: [], where: `${where}:${line.toString()}` })
      continue
    }
    if (text[index] !== ',') {
      unreadable += 1
      continue
    }
    index += 1
    while (/\s/.test(text[index] ?? ''))
      index += 1
    // A variable, a spread, a call: this reads literals only, and reports how many it could not read rather than assuming.
    if (text[index] !== '{') {
      unreadable += 1
      continue
    }
    const end = literalEnd(text, index)
    if (end === undefined) {
      unreadable += 1
      continue
    }
    sites.push({ command, keys: topLevelKeys(text.slice(index, end)), where: `${where}:${line.toString()}` })
  }
  return { sites, unreadable }
}

export function withoutCommentLines(text: string): string {
  return text
    .split('\n')
    .map((line) => {
      const trimmed = line.trimStart()
      const comment = trimmed.startsWith('*') || trimmed.startsWith('//') || trimmed.startsWith('/*')
      return comment ? ' '.repeat(line.length) : line
    })
    .join('\n')
}
