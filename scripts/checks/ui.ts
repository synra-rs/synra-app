import { readFile } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { synraAntdFloatingCss } from '@synra/theme/antd'
import { failures, here, notes, sources } from './common'

export async function checkPageButtons(): Promise<void> {
  const config = await readFile(join(here, 'uno.config.ts'), 'utf8')
  const css = synraAntdFloatingCss()
  if (!config.includes('synraAntdFloatingCss()') || !css.includes('@media (any-pointer: coarse)') || !css.includes('var(--synra-control-touch)'))
    failures.push({ check: 'page-buttons', detail: 'Shared control density and coarse pointer targets must be installed by the theme preflight' })
  notes.push('page-buttons: shared theme owns desktop density and coarse pointer targets')
}

export async function checkDialogNames(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0
  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    let index = text.indexOf('<AModal')
    while (index !== -1) {
      // ★ The opening tag ends at the first `>` that is not inside a quoted value. `indexOf('>')` was the first
      //   version, and it is wrong: `DevicesPage` writes `:get-container="() => document.body"`, so its arrow put a
      //   `>` two attributes before `:title`. It happened to survive because `:title` came first there — the next
      //   dialog to put an arrow ahead of its title would have been reported as unnamed while being named.
      const after = text.slice(index)
      const end = /^(?:"[^"]*"|'[^']*'|[^>"'])*>/.exec(after)
      const close = end ? index + end[0].length - 1 : -1
      const tag = text.slice(index, close === -1 ? undefined : close)
      counted += 1
      if (!/:title=|:wrap-props=/.test(tag)) {
        const line = text.slice(0, index).split('\n').length
        failures.push({
          check: 'dialog-names',
          detail: `${relative(here, file)}:${line.toString()} opens a dialog with neither \`:title\` nor \`:wrap-props=\``
        })
      }
      index = text.indexOf('<AModal', index + 1)
    }
  }
  notes.push(`dialog-names: ${counted.toString()} dialog(s) are named`)
}

export async function checkControlNames(): Promise<void> {
  const files = await sources(join(here, 'src'))
  // ★ `AInput` and `ATextarea` are here because they were the next two that could be unnamed, and one was: the plugin
  //   filter carried a `placeholder` and nothing else. A placeholder is not a name — a screen reader may or may not
  //   announce it, and it disappears the moment there is text, exactly when the user needs to know what the field is.
  //   `ASearchPalette`'s field had both, which is what made the difference visible.
  //
  // ★ `ASwitch` and `ACheckbox` are deliberately absent: every one in the tree is either named by `aria-label` or is a
  //   `<label>`/child that names itself, and a control that names itself by its own text is not something this scan can
  //   judge from the opening tag — asking them for an attribute would be a false failure (the devices page's step
  //   select is the same shape, and it is correct).
  const controls = [
    { tag: 'ASelect', pattern: /<ASelect\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g },
    { tag: 'AInputNumber', pattern: /<AInputNumber\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g },
    { tag: 'AInput', pattern: /<AInput\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g },
    { tag: 'ATextarea', pattern: /<ATextarea\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g }
  ] as const
  const counts: string[] = []
  for (const { tag, pattern } of controls) {
    let counted = 0
    for (const file of files) {
      if (!file.endsWith('.vue'))
        continue
      const text = await readFile(file, 'utf8')
      for (const match of text.matchAll(pattern)) {
        counted += 1
        if (/aria-label|:id=|id=|:aria-labelledby/.test(match[0]))
          continue
        // Named by containment: something between the control and the nearest `<label>` opening tag.
        const before = text.slice(0, match.index)
        const openedLabel = before.lastIndexOf('<label')
        const closedLabel = before.lastIndexOf('</label>')
        if (openedLabel > closedLabel)
          continue
        const line = before.split('\n').length
        failures.push({
          check: 'control-names',
          detail: `${relative(here, file)}:${line.toString()} draws an \`${tag}\` with no \`aria-label\`, no \`id\` and no enclosing label`
        })
      }
    }
    counts.push(`${counted.toString()} ${tag}`)
  }
  notes.push(`control-names: ${counts.join(', ')} are named`)
}

export async function checkTouchBreakpoints(): Promise<void> {
  for (const file of await sources(join(here, 'src'))) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    if (/@media[^{}]*pointer:\\s*coarse[^{}]*max-width|@media[^{}]*max-width[^{}]*pointer:\\s*coarse/.test(text))
      failures.push({ check: 'touch-breakpoints', detail: `${relative(here, file)} must select touch targets by input capability alone` })
  }
  notes.push('touch-breakpoints: touch targets are independent of viewport width')
}

export async function checkCornerLadder(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0
  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    // ★ Comments are stripped first: this rule is about the class the browser gets, and a note that says "not bare
    //   `rounded`" is the opposite of a violation. The strip keeps newlines so positions still map to the file.
    const code = text
      .replace(/<!--[\s\S]*?-->/g, comment => comment.replace(/[^\n]/g, ' '))
      .replace(/\/\*[\s\S]*?\*\//g, comment => comment.replace(/[^\n]/g, ' '))
      .replace(/\/\/[^\n]*/g, comment => comment.replace(/[^\n]/g, ' '))
    for (const match of code.matchAll(/\brounded(?![-:\w])|\brounded-(?:sm|md|lg|xl|2xl|3xl|none)\b/g)) {
      counted += 1
      const line = code.slice(0, match.index).split('\n').length
      failures.push({
        check: 'corner-ladder',
        detail: `${relative(here, file)}:${line.toString()} uses \`${match[0]}\`, which is not on the ladder — use rounded-control, rounded-surface or rounded-pill`
      })
    }
  }
  notes.push(
    counted === 0
      ? 'corner-ladder: every corner comes from the ladder'
      : `corner-ladder: ${counted.toString()} corner(s) off the ladder`
  )
}

export async function checkNativeButtons(): Promise<void> {
  const files = await sources(join(here, 'src'))
  let counted = 0
  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    const code = text.replace(/<!--[\s\S]*?-->/g, comment => comment.replace(/[^\n]/g, ' '))
    // ★ No character cap and no `?>`: the tag ends at the first `>` outside a quoted value. The first version was
    //   `/[\s\S]{0,800}?>/`, which cuts at 800 characters — the longest tag in the tree is 505, so it holds today and
    //   would fail silently tomorrow — and stops at a `>` inside an attribute, the same mistake `dialog-names` and
    //   `control-names` each had (16.155, 16.156).
    for (const match of code.matchAll(/<button\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g)) {
      counted += 1
      const noBorder = /border-0|border-none/.test(match[0])
      const noFill = /bg-transparent/.test(match[0])
      if (!noBorder || !noFill) {
        const line = code.slice(0, match.index).split('\n').length
        const missing = [!noBorder && '`border-0`', !noFill && '`bg-transparent`'].filter(Boolean).join(' and ')
        failures.push({
          check: 'native-buttons',
          detail: `${relative(here, file)}:${line.toString()} draws a native \`<button>\` without ${missing} — the user agent's tile and frame show through`
        })
      }
    }
  }
  notes.push(`native-buttons: ${counted.toString()} native \`<button>\`${counted === 1 ? '' : 's'} clear the user agent look`)
}

export async function checkFocusableNames(): Promise<void> {
  const files = await sources(join(here, 'src'))
  const regionTags = new Set(['section', 'main', 'aside', 'nav'])
  let counted = 0
  for (const file of files) {
    if (!file.endsWith('.vue'))
      continue
    const text = await readFile(file, 'utf8')
    // Quotes first, then the tag end — an attribute value may hold `>`.
    for (const match of text.matchAll(/<(section|main|aside|nav|div|span)\b(?:"[^"]*"|'[^']*'|[^>"'])*>/g)) {
      const tag = match[0]
      const index = /tabindex="(\d+)"/.exec(tag)
      if (!index)
        continue
      // `-1` is a script target, not a tab stop; its own text is its name.
      if (Number(index[1]) < 0)
        continue
      const isRegion = regionTags.has(match[1]!) || /:?role=/.test(tag)
      if (!isRegion)
        continue
      counted += 1
      // `aria-label` also covers `aria-labelledby`: the shorter pattern is a prefix of the longer one.
      if (!/aria-label/.test(tag)) {
        const line = text.slice(0, match.index).split('\n').length
        failures.push({
          check: 'focusable-names',
          detail: `${relative(here, file)}:${line.toString()} makes a region focusable with no \`aria-label\` or \`aria-labelledby\` — a screen reader announces its entire contents`
        })
      }
    }
  }
  notes.push(`focusable-names: ${counted.toString()} focusable region(s) are named`)
}
