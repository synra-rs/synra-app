export const input = {
  title: 'Allow mouse control?',
  request: (device: string) => `${device} wants to control this computer’s mouse.`,
  help: 'Allow movement, clicks, scrolling and dragging for this session. A new session needs your approval again.',
  plugin: 'Plugin',
  allow: 'Allow this session',
  refuse: 'Decline',
  active: (device: string) => `${device} is controlling the mouse`,
  starting: 'Preparing mouse control…',
  stop: 'Stop control',
  indicator: 'Mouse control'
}
