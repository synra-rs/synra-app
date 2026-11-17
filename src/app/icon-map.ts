/**
 * Shared icon vocabulary for navigation and global tools.
 *
 * Data-driven bindings use this map so the shell, search palette and UnoCSS safelist cannot drift apart.
 */
export const icons = {
  pages: {
    home: 'i-carbon-home',
    devices: 'i-carbon-devices',
    plugins: 'i-carbon-application',
    settings: 'i-carbon-settings'
  },
  search: 'i-carbon-search'
} as const
