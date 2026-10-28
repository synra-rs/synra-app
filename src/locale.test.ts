// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { locale, localePreference, normalizePreference, resolveLocale, setLocalePreference, watchSystemLocale } from './locale'
import { formatDateTime, formatTime } from './locale-format'
import { messages } from './messages'
import { englishMessages } from './messages/english'
import { chineseEntries } from './messages/zh-CN'

afterEach(() => {
  vi.restoreAllMocks()
  setLocalePreference('system')
  watchSystemLocale()()
})

it.each([
  ['system', ['zh-CN'], 'zh-CN'],
  ['system', ['zh-Hant-TW'], 'zh-CN'],
  ['system', ['de-DE', 'zh_CN', 'en'], 'zh-CN'],
  ['system', ['en-GB', 'zh-CN'], 'en'],
  ['system', ['fr-FR'], 'en'],
  ['system', [], 'en'],
  ['en', ['zh-CN'], 'en'],
  ['zh-CN', ['en-US'], 'zh-CN']
] as const)('resolves %s with %j to %s', (preference, languages, expected) => {
  expect(resolveLocale(preference, languages)).toBe(expected)
})

it('treats invalid persisted preferences as following the system', () => {
  expect(normalizePreference(null)).toBe('system')
  expect(normalizePreference('fr')).toBe('system')
  expect(normalizePreference('zh-CN')).toBe('zh-CN')
})

it('updates existing computed captions and keeps external names untouched', () => {
  const caption = computed(() => messages.nav.settings.label)
  setLocalePreference('en')
  expect(caption.value).toBe('Settings')
  setLocalePreference('zh-CN')
  expect(caption.value).toBe('设置')
  expect(messages.devices.pairingSelected('My laptop', 'device-1')).toContain('My laptop')
  expect(messages.notices.updated('@acme/hello', '1.2.3')).toContain('@acme/hello')
  expect(messages.nav.settings.icon).toBe(englishMessages.nav.settings.icon)
})

it('follows system changes only when selected and unregisters its listener', () => {
  const languages = vi.spyOn(navigator, 'languages', 'get').mockReturnValue(['en-US'])
  const stop = watchSystemLocale()
  setLocalePreference('system')
  expect(locale.value).toBe('en')
  languages.mockReturnValue(['zh-CN'])
  window.dispatchEvent(new Event('languagechange'))
  expect(locale.value).toBe('zh-CN')
  setLocalePreference('en')
  window.dispatchEvent(new Event('languagechange'))
  expect(locale.value).toBe('en')
  stop()
  languages.mockReturnValue(['en-US'])
  window.dispatchEvent(new Event('languagechange'))
  setLocalePreference('system')
  expect(locale.value).toBe('zh-CN')
  expect(localePreference.value).toBe('system')
})

it('covers every text key and preserves template placeholders', () => {
  const keys: string[] = []
  function visit(tree: Readonly<Record<string, unknown>>, prefix = ''): void {
    for (const [key, value] of Object.entries(tree)) {
      if (key === 'icon')
        continue
      const path = `${prefix}${key}`
      if (value !== null && typeof value === 'object') {
        visit(value as Readonly<Record<string, unknown>>, `${path}.`)
        continue
      }
      keys.push(path)
      const translated = chineseEntries[path as keyof typeof chineseEntries]
      expect(typeof translated, path).toBe(typeof value)
      if (typeof value === 'string' && typeof translated === 'string')
        expect(translated.match(/\{\w+\}/g) ?? [], path).toEqual(value.match(/\{\w+\}/g) ?? [])
    }
  }
  visit(englishMessages)
  expect(Object.keys(chineseEntries).sort()).toEqual(keys.sort())
})

it('reformats existing dates when the language changes', () => {
  const at = new Date(2026, 9, 29, 13, 7, 9).valueOf()
  const time = computed(() => formatTime(at, true))
  setLocalePreference('en')
  const english = time.value
  expect(formatDateTime(at)).toContain('2026')
  setLocalePreference('zh-CN')
  expect(time.value).toBe('13:07')
  expect(time.value).not.toBe(english)
  expect(formatDateTime(at)).toContain('2026')
})
