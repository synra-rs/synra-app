<template>
  <AModal :get-container="false" :open="visible" :width="580" :title="messages.plugins.workflow.localInstall" :footer="null" :closable="!installing && !choosing" :mask-closable="!installing && !choosing" :keyboard="!installing && !choosing" @cancel="$emit('close')">
    <div class="stack gap-4">
      <p class="m-0 text-base text-ink-dim">
        {{ messages.plugins.workflow.localHelp }}
      </p>
      <div class="row flex-wrap gap-2">
        <AButton type="primary" :loading="choosing" :disabled="installing" @click="choose(false)">
          {{ messages.plugins.choosePackage }}
        </AButton>
        <AButton :loading="choosing" :disabled="installing" @click="choose(true)">
          {{ messages.plugins.chooseDirectory }}
        </AButton>
      </div>
      <p v-if="path" class="m-0 break-all text-base font-mono">
        {{ path }}
      </p>
      <details>
        <summary class="synra-choice min-h-8 cursor-pointer text-sm text-ink-dim">
          {{ messages.plugins.workflow.manualPath }}
        </summary>
        <AInput v-model:value="path" class="mt-2" :aria-label="messages.plugins.installPath" :placeholder="messages.plugins.installPath" :disabled="installing || choosing" @keydown="guardCompositionEscape" @press-enter="submit" />
      </details>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton :disabled="installing || choosing" @click="$emit('close')">
          {{ messages.plugins.workflow.cancel }}
        </AButton>
        <AButton type="primary" :loading="installing" :disabled="!path.trim() || choosing" data-testid="install-submit" @click="install">
          {{ messages.plugins.installAction }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { InstallReport } from '@synra/protocol'
import type { Failure } from './actions'
import { Button as AButton, Input as AInput, Modal as AModal } from 'ant-design-vue'
import { onScopeDispose, ref } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { kernel } from '../../kernel'
import { guardCompositionEscape, isComposing } from '../../keyboard'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'
import { installFrom } from './actions'

const emit = defineEmits<{ close: [], installed: [report: InstallReport] }>()
const visible = useDialogVisibility(ref(true), 40)
const path = ref('')
const choosing = ref(false)
const installing = ref(false)
const error = ref<Failure>()
let alive = true
onScopeDispose(() => alive = false)
async function choose(directory: boolean): Promise<void> {
  if (choosing.value || installing.value)
    return
  choosing.value = true
  error.value = undefined
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory, multiple: false, title: directory ? messages.plugins.chooseDirectory : messages.plugins.choosePackage, ...(!directory ? { filters: [{ name: 'Synra', extensions: ['synra'] }] } : {}) })
    if (alive && typeof selected === 'string')
      path.value = selected
  }
  catch {
    if (alive)
      error.value = { code: 'internal', message: 'error.internal' }
  }
  finally {
    choosing.value = false
  }
}
function submit(event: KeyboardEvent): void {
  if (!isComposing(event))
    void install()
}
async function install(): Promise<void> {
  if (!path.value.trim() || installing.value || choosing.value)
    return
  installing.value = true
  error.value = undefined
  try {
    const answer = await installFrom(kernel, path.value.trim())
    if (!alive)
      return
    if (answer.ok)
      emit('installed', answer.value)
    else
      error.value = answer.failure
  }
  finally {
    installing.value = false
  }
}
</script>
