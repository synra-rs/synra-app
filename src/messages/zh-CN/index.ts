import type { LocaleEntries } from '../types'
import { buildLocale } from '../build-locale'
import { common } from './common'
import { devices } from './devices'
import { home } from './home'
import { plugins } from './plugins'
import { search } from './search'
import { settings } from './settings'

export const chineseEntries = { ...common, ...devices, ...home, ...plugins, ...search, ...settings } satisfies LocaleEntries
export const chineseMessages = buildLocale(chineseEntries)
