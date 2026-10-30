/**
 * The shell's one search box.
 *
 * Two things it says, and nothing else: what it looks through, and what it found nothing of. The results themselves
 * are object names the interface already shows — a search result that invented its own wording would be a second name
 * for the same plugin.
 */
export const search = {
  /** The icon's accessible name, and the dialog's title once it opens. */
  title: 'Search',
  placeholder: 'Search plugins, settings and devices',
  /** The hint on the entry, because the shortcut is only useful if it can be found. */
  shortcut: '/',
  /**
   * What is searched, said once. A person who types a device name and sees only settings needs to know the box is
   * meant for all three rather than that the device is missing.
   */
  scope: 'Plugins, settings categories and this device.',
  empty: 'Nothing matches that.',
  /** The group headings, so a flat list of mixed objects is still readable. */
  groups: {
    plugins: 'Plugins',
    settings: 'Settings',
    devices: 'Devices',
    pages: 'Pages'
  }
}
