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
    offlineDetail: 'Device communication may be limited until the network returns.',
    /**
     * Names the scrollable `main` that holds `RouterView`.
     *
     * It is focusable (`tabindex="0"`, so a keyboard user can scroll it) and every focusable region needs a name —
     * without one a screen reader announces the whole page text as the region's label, which is what
     * `bsk_inspect.observe` showed: `main "首页 设备连接 已连接 This device Pixel 8 正在传输 …"`.
     */
    content: 'Page content'
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
