<template>
  <AModal :open="visible" :width="600" :title="messages.plugins.workflow.details" :footer="null" :closable="!installing" :mask-closable="!installing" :keyboard="!installing" @cancel="$emit('close')">
    <div class="stack gap-4">
      <p v-if="loading" class="m-0 text-base text-ink-dim" role="status">
        {{ messages.plugins.workflow.loading }}
      </p>
      <template v-if="preview">
        <h3 class="m-0 text-lg font-semibold">
          {{ preview.title }}
        </h3>
        <p v-if="preview.description" class="m-0 text-base">
          {{ preview.description }}
        </p>
        <dl class="grid grid-cols-[auto_minmax(0,1fr)] m-0 gap-2 text-base">
          <dt class="text-ink-dim">
            {{ messages.plugins.workflow.source }}
          </dt><dd class="m-0">
            {{ messages.plugins.workflow.registry }}
          </dd>
          <dt class="text-ink-dim">
            {{ messages.plugins.pluginId }}
          </dt><dd class="m-0 break-all font-mono">
            {{ preview.pluginId }}
          </dd>
          <dt class="text-ink-dim">
            {{ messages.plugins.version }}
          </dt><dd class="m-0">
            {{ preview.version }}
          </dd>
          <dt class="text-ink-dim">
            {{ messages.plugins.publisher }}
          </dt><dd class="m-0 break-all">
            {{ preview.publisherName || preview.publisher }}
          </dd>
        </dl>
        <p v-if="preview.publisherKey" class="m-0 break-all text-sm text-ink-dim">
          {{ messages.plugins.preview.directoryKey }}: <code>{{ preview.publisherKey }}</code>
        </p>
        <div v-for="capability in preview.capabilities" :key="capability" class="stack gap-1 border-b border-line pb-2">
          <span class="text-base">{{ capabilityPurpose(capability) }}</span><code class="break-all text-sm text-ink-dim">{{ capability }}</code>
        </div>
        <p v-if="!preview.capabilities.length" class="m-0 text-base text-ink-dim">
          {{ messages.plugins.preview.none }}
        </p>
        <p class="m-0 text-sm text-ink-dim">
          {{ preview.hasUi ? messages.plugins.preview.ui : messages.plugins.preview.headless }}
        </p>
        <p v-if="preview.sizeBytes !== undefined" class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.size.replace('{size}', formatBytes(preview.sizeBytes)) }}
        </p>
      </template>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton :disabled="installing" @click="$emit('close')">
          {{ messages.plugins.workflow.close }}
        </AButton>
        <AButton v-if="error && !preview" :loading="loading" @click="load">
          {{ messages.actions.retry }}
        </AButton>
        <AButton type="primary" :disabled="!preview || loading" :loading="installing" @click="install">
          {{ messages.plugins.installAction }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { InstallPreviewDto, InstallReport } from '@synra/protocol'
import type { Failure } from './actions'
import { Button as AButton, Modal as AModal } from 'ant-design-vue'
import { onScopeDispose, ref, watch } from 'vue'
import { kernel } from '../../app/kernel'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'
import { formatBytes } from '../../utils/format'
import { installFromRegistry, previewInstall } from './actions'
import { capabilityPurpose } from './capabilities'

const props = defineProps<{ pluginId: string }>()
const emit = defineEmits<{ close: [], installed: [report: InstallReport] }>()
const visible = useDialogVisibility(ref(true), 40)
const preview = ref<InstallPreviewDto>()
const error = ref<Failure>()
const loading = ref(false)
const installing = ref(false)
let revision = 0
onScopeDispose(() => revision++)
async function load(): Promise<void> {
  const ticket = ++revision
  preview.value = undefined
  error.value = undefined
  loading.value = true
  const answer = await previewInstall(kernel, props.pluginId)
  if (ticket !== revision)
    return
  loading.value = false
  if (answer.ok)
    preview.value = answer.value
  else
    error.value = answer.failure
}
watch(() => props.pluginId, load, { immediate: true })
async function install(): Promise<void> {
  const chosen = preview.value
  if (!chosen || installing.value || loading.value)
    return
  const ticket = revision
  installing.value = true
  error.value = undefined
  const answer = await installFromRegistry(kernel, chosen.pluginId)
  if (ticket !== revision)
    return
  installing.value = false
  if (answer.ok)
    emit('installed', answer.value)
  else
    error.value = answer.failure
}
</script>
