import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { effectScope, ref } from 'vue'
import { useDialogVisibility } from '@/stores/dialogs'

beforeEach(() => setActivePinia(createPinia()))
describe('dialog presentation', () => {
  it('suspends an editor for pairing and restores the same draft afterwards', () => {
    const scope = effectScope()
    scope.run(() => {
      const editorOpen = ref(true)
      const pairingOpen = ref(false)
      const draft = ref('a chosen local source')
      const editor = useDialogVisibility(editorOpen, 40)
      const pairing = useDialogVisibility(pairingOpen, 100)
      expect(editor.value).toBe(true)
      pairingOpen.value = true
      expect(editor.value).toBe(false)
      expect(pairing.value).toBe(true)
      expect(editorOpen.value).toBe(true)
      pairingOpen.value = false
      expect(editor.value).toBe(true)
      expect(draft.value).toBe('a chosen local source')
    })
    scope.stop()
  })
  it('releases a presentation when its owner is unmounted', () => {
    const editorScope = effectScope()
    const pairingScope = effectScope()
    const editor = editorScope.run(() => useDialogVisibility(ref(true), 40))!
    pairingScope.run(() => useDialogVisibility(ref(true), 100))
    expect(editor.value).toBe(false)
    pairingScope.stop()
    expect(editor.value).toBe(true)
    editorScope.stop()
  })
})
