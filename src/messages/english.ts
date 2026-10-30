import { crash } from './crash'
import { devices } from './devices'
import { actions, errors } from './errors'
import { home } from './home'
import { nav } from './nav'
import { notices } from './notices'
import { plugins } from './plugins'
import { search } from './search'
import { settings } from './settings'
import { transfers } from './transfers'

export type { ErrorCode } from './errors'

export const englishMessages = {
  app: {
    name: 'Synra',
    offline: 'Network unavailable.',
    offlineDetail: 'Device communication may be limited until the network returns.'
  },

  nav,

  notices,

  home,

  devices,

  crash,

  plugins,

  settings,

  search,

  transfers,

  errors,

  actions
} as const
