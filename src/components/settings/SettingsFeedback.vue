<template>
  <!--
    ★ The floor is stated here as well as on `SettingsPage`'s root section. While this sits inside that page the two are
      redundant — and this one is the reason the settings column's error actions are reachable at all. Stating it here
      means the component does not depend on a caller four levels up, which is precisely how `ErrorNotice` became the
      one control a thumb could miss (16.151).
  -->
  <div v-if="error || saved || checkRequired" class="stack gap-2 min-w-0" aria-live="polite">
    <ErrorNotice v-if="error" :code="error.code" :params="error.params" :detail="registryDetail(registryProblem(error.details))" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
    <span v-else-if="saved" class="text-sm text-ink-dim">{{ messages.settings.edit.saved }}</span>
    <template v-if="checkRequired">
      <span class="text-sm text-ink-dim">{{ messages.settings.edit.checkDetail }}</span>
      <AButton :loading="busy" @click="emit('check')">
        {{ messages.settings.edit.check }}
      </AButton>
    </template>
  </div>
</template>

<script setup lang="ts">
import type { IpcError } from '@synra/protocol'
import { Button as AButton } from 'ant-design-vue'
import { registryDetail, registryProblem } from '../../features/plugins/registry-problem'
import { messages } from '../../messages'
import ErrorNotice from '../ErrorNotice.vue'

defineProps<{ error?: IpcError | undefined, saved?: boolean | undefined, checkRequired?: boolean | undefined, busy?: boolean | undefined }>()
const emit = defineEmits<{ check: [] }>()
</script>
