/**
 * The four pages, as the navigation shows them.
 *
 * `router.ts` reads these, so the nav cannot disagree with itself.
 *
 * ★ **The icon name lives here, with the words, because of a real bug it fixed.** It used to be a literal in
 *   `router.ts`, and two of the four icons never rendered: UnoCSS's extractor cannot see a class name that only ever
 *   appears as a *value*, so `i-carbon-home` and `i-carbon-settings` were never generated — while the other two were,
 *   because the pages happened to name them literally in their empty states. `uno.config.ts` derives its `safelist`
 *   from this object, so adding a page here is the only edit that is ever needed.
 *
 * ★ **No hint, no description: a navigation item is its label and its icon.** The first version carried a second line
 *   explaining each page, which made every item a small paragraph and the sidebar a wall of text — and an item you
 *   have to read twice before clicking is an item that slowed you down.
 */
export const nav = {
  home: { label: 'Home', icon: 'i-carbon-home' },
  devices: { label: 'Devices', icon: 'i-carbon-devices' },
  plugins: { label: 'Plugins', icon: 'i-carbon-application' },
  settings: { label: 'Settings', icon: 'i-carbon-settings' }
} as const
