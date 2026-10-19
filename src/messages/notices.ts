/**
 * The words for the kernel's page-less notices.
 *
 * Keyed by the kernel's message key, not by the event kind: one kind (`notice`) can carry several messages, and a switch
 * on the kind would make the second one impossible to add without changing the event.
 */
export const notices = {
  received: (name: string): string => `Received ${name}.`,
  savedTo: (path: string): string => `Saved to ${path}`,
  /** A `synra://` link this build does not understand; it focused the window but did nothing. */
  unknownLink: (url: string): string => `This build does not understand this link: ${url}`,
  /** A notice key this build does not know still has to say something rather than print the key. */
  generic: 'The kernel has something to say.'
} as const
