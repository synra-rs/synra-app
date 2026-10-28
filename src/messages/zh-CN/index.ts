import type { LocaleEntries } from '../types'
import { buildLocale } from '../build-locale'
import { common } from './common'
import { devices } from './devices'
import { home } from './home'
import { plugins } from './plugins'
import { settings } from './settings'

export const chineseEntries = { ...common, ...devices, ...home, ...plugins, ...settings } satisfies LocaleEntries
export const chineseMessages = buildLocale(chineseEntries)
