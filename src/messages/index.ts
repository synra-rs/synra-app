import type { Messages } from './types'
import { locale } from '../locale'
import { englishMessages } from './english'
import { chineseMessages } from './zh-CN'

export type { ErrorCode } from './errors'

const catalogs: Record<'en' | 'zh-CN', Messages> = { 'en': englishMessages, 'zh-CN': chineseMessages }

/** Existing callers keep typed keys; render effects subscribe to the selected language. */
export const messages: Messages = new Proxy(englishMessages, {
  get: (_, key) => Reflect.get(catalogs[locale.value], key)
})
