/**
 * The startup crash notice's words.
 *
 * One name per action, because this banner is the first thing a user sees after a crash and it has three buttons. "Ignore"
 * is the word the design uses, and it is the honest one: the reports are deleted, not archived.
 */
export const crash = {
  title: 'Synra ended unexpectedly last time.',
  detail: (count: number): string =>
    count === 1
      ? 'One crash report is from the run that just ended. Plugins may have been left switched off.'
      : `${count} crash reports are from the run that just ended. Plugins may have been left switched off.`,
  showDetails: 'Show details',
  hideDetails: 'Hide details',
  exportBundle: 'Export diagnostics',
  ignore: 'Ignore',
  noMessage: 'the panic carried no message'
} as const
