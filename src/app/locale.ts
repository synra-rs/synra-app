import { computed, ref, shallowRef } from 'vue'

export type Locale = 'en' | 'zh-CN'
export type LocalePreference = Locale | 'system'

export function normalizePreference(value: unknown): LocalePreference {
  return value === 'en' || value === 'zh-CN' ? value : 'system'
}

export function resolveLocale(preference: LocalePreference, languages: readonly string[]): Locale {
  if (preference !== 'system')
    return preference
  for (const language of languages) {
    const base = language.toLowerCase().replaceAll('_', '-').split('-')[0]
    if (base === 'zh')
      return 'zh-CN'
    if (base === 'en')
      return 'en'
  }
  return 'en'
}

function systemLanguages(): readonly string[] {
  return typeof navigator === 'undefined' ? [] : navigator.languages.length ? navigator.languages : [navigator.language]
}

export const localePreference = ref<LocalePreference>('system')
const languages = shallowRef(systemLanguages())
export const locale = computed(() => resolveLocale(localePreference.value, languages.value))

export function setLocalePreference(value: unknown): void {
  localePreference.value = normalizePreference(value)
}

/** Registered by the root component, so HMR and remounts cannot leak listeners. */
export function watchSystemLocale(): () => void {
  const refresh = (): void => {
    languages.value = systemLanguages()
  }
  refresh()
  window.addEventListener('languagechange', refresh)
  return () => window.removeEventListener('languagechange', refresh)
}
