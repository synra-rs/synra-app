<template>
  <!--
    ★ The floor is stated here as well as on `SettingsPage`'s root section. While this sits inside that page the two are
      redundant — and this one is the reason the settings column's error actions are reachable at all. Stating it here
      means the component does not depend on a caller four levels up, which is precisely how `ErrorNotice` became the
      one control a thumb could miss (16.151).
  -->
  <div v-if="error || saved || checkRequired" class="min-w-0 stack gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8" aria-live="polite">
    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
    <span v-else-if="saved" class="text-xs text-ink-dim">{{ messages.settings.edit.saved }}</span>
    <template v-if="checkRequired">
      <span class="text-xs text-ink-dim">{{ messages.settings.edit.checkDetail }}</span>
      <AButton :loading="busy" @click="emit('check')">
        {{ messages.settings.edit.check }}
      </AButton>
    </template>
  </div>
</template>

<script setup lang="ts">
import type { IpcError } from '@synra/protocol'
import { Button as AButton } from 'ant-design-vue'
import { messages } from '../../messages'
import ErrorNotice from '../ErrorNotice.vue'

defineProps<{ error?: IpcError | undefined, saved?: boolean | undefined, checkRequired?: boolean | undefined, busy?: boolean | undefined }>()
const emit = defineEmits<{ check: [] }>()
</script>
