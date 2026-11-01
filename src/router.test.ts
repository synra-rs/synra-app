// @vitest-environment jsdom
import { expect, it } from 'vitest'
import { setLocalePreference } from './locale'
import { messages } from './messages'
import { PAGES, router } from './router'

/**
 * `router.ts` had no test.
 *
 * The file's own note says the nav is declared as data "so that the nav and the router cannot disagree — a
 * hand-written nav is where the 'Devices' link that goes nowhere comes from". That promise is only true if something
 * checks it. `PAGES` drives the sidebar *and* the route table, but nothing verified the two stayed in step after a
 * fifth page is added, or that a route written by hand did not appear beside them.
 *
 * The label getters are the other half: each one reads `messages` at access time, so a language change has to be
 * visible through `PAGES` without rebuilding the array. A plain `label: messages.nav.home.label` would freeze the
 * first language the module was imported under.
 */

it('gives every page a route, and no route a page that is not in the nav', () => {
  const routed = router.getRoutes().map(route => route.path).sort()
  const navigable = PAGES.map(page => page.path).sort()
  expect(routed).toEqual(navigable)
})

it('puts home first, because it is where the shell opens', () => {
  expect(PAGES[0]?.path).toBe('/')
})

it('keeps the four paths distinct', () => {
  // A duplicate path would make one route unreachable while still filling the sidebar.
  const paths = PAGES.map(page => page.path)
  expect(new Set(paths).size, `duplicate path in ${paths.join(', ')}`).toBe(paths.length)
})

it('reads labels through messages, so a language change reaches the nav', () => {
  setLocalePreference('en')
  try {
    expect(PAGES.map(page => page.label)).toEqual([
      messages.nav.home.label,
      messages.nav.devices.label,
      messages.nav.plugins.label,
      messages.nav.settings.label
    ])
    // The point is that these are the *current* strings, not the ones at import time.
    expect(PAGES[0]?.label).toBe('Home')
    setLocalePreference('zh-CN')
    expect(PAGES[0]?.label).toBe('首页')
  }
  finally {
    setLocalePreference('system')
  }
})

it('names every page with an icon the stylesheet can extract', () => {
  // R41 inlines icons at build time; an entry that is empty or not a Carbon class renders as a blank square.
  for (const page of PAGES) {
    expect(page.icon, `${page.path} has no icon`).toBeTruthy()
    expect(page.icon, `${page.path} icon is not a Carbon class`).toMatch(/^i-carbon-/)
  }
})
