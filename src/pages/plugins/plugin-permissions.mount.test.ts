// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The permission checkboxes had no name as a group.
 *
 * `ACheckboxGroup` renders `role="group"`; a group with no `aria-label`/`aria-labelledby` is announced as "group" and
 * nothing else. A screen reader arriving at the list of capabilities a plugin asks for heard each box and its own
 * label, but never what the set was *for* — while the heading that answers exactly that was already on screen, one
 * element above, attached to nothing.
 *
 * Read as source, not mounted: the group sits four levels inside a plugin row that needs a store, a router and a
 * kernel to render, and the question here is only whether the label and the reference resolve to the same id. That is
 * a question about one expression appearing twice, which is exactly what would drift if someone edited one of them.
 */

const PAGE = join(__dirname, '..', '..', 'pages', 'PluginsPage.vue')
const source = readFileSync(PAGE, 'utf8')

/** Every opening tag of the given name, quote-aware so an attribute value holding `>` does not end the match early. */
function tags(name: string): string[] {
  const pattern = new RegExp(`<${name}\\b(?:"[^"]*"|'[^']*'|[^>"'])*>`, 'g')
  return [...source.matchAll(pattern)].map(match => match[0])
}

describe('plugin permission group', () => {
  it('names the group from the heading above it', () => {
    const groups = tags('ACheckboxGroup')
    expect(groups.length, 'no ACheckboxGroup on the page').toBeGreaterThan(0)
    for (const opening of groups) {
      expect(
        /:?aria-labelledby="/.test(opening) || /:?aria-label="/.test(opening),
        `a permission group is unnamed:\n${opening}`
      ).toBe(true)
    }
  })

  it('points at an id the label above actually carries', () => {
    // The group binds `:aria-labelledby="requestedLabelId(plugin.pluginId)"`, and the heading above it binds the same
    // expression into `id`. Two expressions that must agree; a source read is what can see them together.
    const call = 'requestedLabelId(plugin.pluginId)'
    const bound = source.split(call).length - 1
    expect(bound, `\`${call}\` is not bound exactly twice (id and reference)`).toBe(2)

    // Both bindings have to be on the two elements that need them, not somewhere else on the page.
    const heading = tags('span').find(opening => opening.includes(`:id="${call}"`))
    expect(heading, 'no span carries the heading id').toBeDefined()
    // The text sits between the tags, so the check is on what follows the opening one.
    const after = source.slice(source.indexOf(heading!) + heading!.length, source.indexOf(heading!) + heading!.length + 120)
    expect(after, 'the heading id is not on the requested-permissions text').toContain('messages.plugins.requested')

    const group = tags('ACheckboxGroup').find(opening => opening.includes(`:aria-labelledby="${call}"`))
    expect(group, 'no group references that id').toBeDefined()
  })

  it('derives the id from the plugin, so rows do not name each other', () => {
    // A fixed id would leave every group after the first pointing at another plugin's heading.
    const declared = /function requestedLabelId\([^)]*\)[^{]*\{([\s\S]{0,200}?)\n\}/.exec(source)
    expect(declared, 'requestedLabelId is not a local function').not.toBeNull()
    expect(declared![1], 'the id does not depend on the plugin').toMatch(/pluginId/)
  })

  it('sanitises the id, because a plugin id contains a scope', () => {
    // `@synra-plugin/github` is a legal id and an illegal fragment in an `id` attribute; it has to be flattened.
    const declared = /function requestedLabelId\([^)]*\)[^{]*\{([\s\S]{0,200}?)\n\}/.exec(source)!
    expect(declared[1], 'the id keeps characters that are not valid in an id').toMatch(/replace\(/)
  })
})
