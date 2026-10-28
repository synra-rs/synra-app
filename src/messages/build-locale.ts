import type { LocaleEntries, Messages } from './types'
import { englishMessages } from './english'

/** Icons stay shared; a missing or mistyped translation fails during development and tests. */
export function buildLocale(entries: LocaleEntries): Messages {
  const translations = entries as Readonly<Record<string, unknown>>
  function visit(source: Readonly<Record<string, unknown>>, prefix = ''): Record<string, unknown> {
    return Object.fromEntries(Object.entries(source).map(([key, value]) => {
      const path = `${prefix}${key}`
      if (key === 'icon')
        return [key, value]
      if (value !== null && typeof value === 'object')
        return [key, visit(value as Readonly<Record<string, unknown>>, `${path}.`)]
      const translated = translations[path]
      if (typeof translated !== typeof value)
        throw new TypeError(`Invalid translation: ${path}`)
      return [key, translated]
    }))
  }
  return visit(englishMessages) as Messages
}
