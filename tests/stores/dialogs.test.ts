import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { dismissActiveDialog, useDialogVisibility } from '@/stores/dialogs'

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

describe('dialog back coordination', () => {
  it('dismisses the highest priority visible dialog first', () => {
    const scope = effectScope()
    const result = scope.run(() => {
      const lowerOpen = ref(false)
      const higherOpen = ref(false)
      const lowerDismiss = vi.fn(() => {
        lowerOpen.value = false
      })
      const higherDismiss = vi.fn(() => {
        higherOpen.value = false
      })
      const lowerVisible = useDialogVisibility(lowerOpen, 10, lowerDismiss)
      const higherVisible = useDialogVisibility(higherOpen, 20, higherDismiss)
      lowerOpen.value = true
      higherOpen.value = true
      return { lowerOpen, higherOpen, lowerDismiss, higherDismiss, lowerVisible, higherVisible }
    })!

    expect(result.lowerVisible.value).toBe(false)
    expect(result.higherVisible.value).toBe(true)
    expect(dismissActiveDialog()).toBe(true)
    expect(result.higherDismiss).toHaveBeenCalledOnce()
    expect(result.higherOpen.value).toBe(false)
    expect(result.lowerVisible.value).toBe(true)
    expect(dismissActiveDialog()).toBe(true)
    expect(result.lowerDismiss).toHaveBeenCalledOnce()
    expect(result.lowerOpen.value).toBe(false)
    expect(dismissActiveDialog()).toBe(false)
    scope.stop()
  })

  it('consumes back when a visible dialog has no safe dismiss action', () => {
    const scope = effectScope()
    const open = ref(true)
    const visible = scope.run(() => useDialogVisibility(open, 1))!
    expect(visible.value).toBe(true)
    expect(dismissActiveDialog()).toBe(true)
    expect(open.value).toBe(true)
    scope.stop()
  })
})
