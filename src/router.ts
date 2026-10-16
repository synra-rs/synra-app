/**
 * The four screens, and the order they are in.
 *
 * # Why a router at all in M0
 *
 * Because the shell's hardest problem is not the pages — it is the seam between them. A design where every screen is
 * reachable at once by a link (or by nothing) defers the question of what survives navigation until M4, when there is a
 * real app around it. Four routes and a nav is cheap now and is the thing that has to exist before any of it is knowable.
 *
 * # Why these four
 *
 * The design document's page list: home, devices, plugins, settings. Each is named by what it is *for*, not by what it
 * shows — "devices" is where you pair and send, "plugins" is where you install and grant, and "home" is the thing you
 * see when nothing in particular is wrong.
 *
 * `hash` history is deliberate rather than a placeholder: the Tauri shell serves the built assets over its own
 * protocol, where a deep link is a file read, and the hash router needs no server rewrite to survive a reload.
 */

import { createRouter, createWebHashHistory } from 'vue-router'

import DevicesPage from './pages/DevicesPage.vue'
import HomePage from './pages/HomePage.vue'
import PluginsPage from './pages/PluginsPage.vue'
import SettingsPage from './pages/SettingsPage.vue'

/**
 * One entry in the sidebar.
 *
 * Declared as data rather than written into the template so that the nav and the router cannot disagree — a hand-written
 * nav is where the "Devices" link that goes nowhere comes from.
 */
export interface PageRoute {
  /** The path, and the name. */
  readonly path: string
  /** What the sidebar says. */
  readonly label: string
  /** One line under the label, so a screen's purpose is visible before it is opened. */
  readonly hint: string
  /**
   * The Carbon icon class, for the rail (md) and the bottom bar (base).
   *
   * Carbon rather than a generic set because it is drawn by the same people as the interface's typeface (R18) — the
   * two are meant to sit together. It has to be an **icon font class, not an image or an inline SVG**, because R41
   * requires icons to be inlined at build time: the window's CSP has `connect-src 'none'`, so an icon fetched at
   * runtime fails as an empty box that looks like a broken icon library.
   */
  readonly icon: string
  /** The component. */
  readonly component: typeof HomePage
}

/** Every page, in sidebar order. */
export const PAGES: readonly PageRoute[] = [
  {
    path: '/',
    label: 'Home',
    hint: 'What this device is doing',
    icon: 'i-carbon-home',
    component: HomePage
  },
  {
    path: '/devices',
    label: 'Devices',
    hint: 'Pair and send',
    icon: 'i-carbon-devices',
    component: DevicesPage
  },
  {
    path: '/plugins',
    label: 'Plugins',
    hint: 'Install and grant',
    icon: 'i-carbon-application',
    component: PluginsPage
  },
  {
    path: '/settings',
    label: 'Settings',
    hint: 'This device',
    icon: 'i-carbon-settings',
    component: SettingsPage
  }
]

/** The router the shell mounts. */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: PAGES.map(page => ({ path: page.path, component: page.component }))
})
