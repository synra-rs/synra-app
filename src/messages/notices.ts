/**
 * The words for the kernel's page-less notices.
 *
 * Keyed by the kernel's message key, not by the event kind: one kind (`notice`) can carry several messages, and a switch
 * on the kind would make the second one impossible to add without changing the event.
 */
export const notices = {
  received: (name: string): string => `Received ${name}.`,
  savedTo: (path: string): string => `Saved to ${path}`,
  /** A package opened from the desktop has landed; it is installed but not trusted or enabled. */
  installed: (pluginId: string, version: string): string => `Installed ${pluginId} ${version}.`,
  /** A newer package replaced the version that was installed. */
  updated: (pluginId: string, version: string): string => `Updated ${pluginId} to ${version}.`,
  /** The same version is already installed; double-clicking it again changed nothing. */
  alreadyInstalled: (pluginId: string, version: string): string => `${pluginId} ${version} is already installed.`,
  /** The package is older than what is installed; a double-click is not an instruction to downgrade. */
  newerInstalled: (pluginId: string, installed: string, offered: string): string =>
    `${pluginId} ${installed} is newer than ${offered}; nothing was replaced.`,
  /** A `synra://` link this build does not understand; it focused the window but did nothing. */
  unknownLink: (url: string): string => `This build does not understand this link: ${url}`,
  /** A notice key this build does not know still has to say something rather than print the key. */
  generic: 'The kernel has something to say.',
  /** The action on a package notice: the next step is on the plugins page. */
  review: 'Open plugins'
} as const
