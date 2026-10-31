<template>
  <section class="stack gap-3" aria-labelledby="credential-title">
    <ACollapse ghost v-bind="expanded || credentials.error || credentials.snapshot?.recovery || credentials.restartRequired ? { activeKey: ['identity'] } : {}">
      <ACollapse.Panel key="identity" force-render>
        <template #header>
          <h2 id="credential-title" class="m-0 text-base font-medium">
            {{ messages.settings.credentials.title }}
          </h2>
        </template>
        <div class="stack gap-3">
          <HelpText>
            {{ messages.settings.credentials.detail }}
          </HelpText>
          <ErrorNotice v-if="credentials.error" :code="credentials.error.code" :actions="credentials.error.actions" @retry="credentials.reload()" />
          <AButton :disabled="credentials.busy" @click="credentials.reload()">
            {{ messages.settings.credentials.refresh }}
          </AButton>
          <template v-if="credentials.snapshot">
            <ADescriptions :column="1" bordered>
              <ADescriptions.Item :label="messages.settings.credentials.fingerprint">
                <span class="break-all font-mono">{{ target?.fingerprint ?? messages.settings.credentials.absent }}</span>
              </ADescriptions.Item>
              <ADescriptions.Item :label="messages.settings.credentials.backend">
                {{ target?.backend ?? messages.settings.credentials.absent }}
              </ADescriptions.Item>
              <ADescriptions.Item :label="messages.settings.credentials.enrollment">
                {{ credentials.snapshot.enrollment?.ready ? messages.settings.credentials.recorded : messages.settings.credentials.incomplete }}
              </ADescriptions.Item>
              <ADescriptions.Item :label="messages.settings.credentials.runtime">
                {{ credentials.snapshot.runtimeReady ? messages.settings.credentials.loaded : messages.settings.credentials.unavailable }}
              </ADescriptions.Item>
              <ADescriptions.Item :label="messages.settings.credentials.retired">
                {{ credentials.snapshot.retiredKeys }}
              </ADescriptions.Item>
            </ADescriptions>
            <AAlert v-if="credentials.snapshot.recovery" type="warning" show-icon :message="messages.settings.credentials.registrationLost" />
            <AAlert v-if="credentials.restartRequired" type="warning" show-icon :message="messages.settings.credentials.restart" />
            <AAlert v-if="credentials.repaired" type="success" show-icon :message="messages.settings.credentials.repaired" />
            <AAlert v-if="credentials.cleanup?.remaining" type="warning" show-icon :message="messages.settings.credentials.remaining(credentials.cleanup.remaining)" />
            <ErrorNotice v-if="credentials.cleanup?.errorCode" :code="credentials.cleanup.errorCode" />
            <div class="row flex-wrap gap-2">
              <AButton danger :disabled="credentials.busy || !target || credentials.repaired !== null" @click="confirmRepair">
                {{ messages.settings.credentials.repair }}
              </AButton>
              <AButton :loading="credentials.busy" :disabled="credentials.busy || credentials.snapshot.retiredKeys === 0" @click="credentials.retryCleanup()">
                {{ messages.settings.credentials.cleanup }}
              </AButton>
            </div>
          </template>
          <AModal :open="confirming !== null" :title="messages.settings.credentials.confirmTitle" :ok-text="messages.settings.credentials.confirm" :cancel-text="messages.settings.field.cancel" :confirm-loading="credentials.busy" :closable="!credentials.busy" :mask-closable="!credentials.busy" :keyboard="!credentials.busy" wrap-class-name="[&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8" @ok="repair" @cancel="close">
            <p>{{ messages.settings.credentials.confirmDetail }}</p>
            <p class="break-all font-mono">
              {{ confirming }}
            </p>
          </AModal>
        </div>
      </ACollapse.Panel>
    </ACollapse>
  </section>
</template>

<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Collapse as ACollapse, Descriptions as ADescriptions, Modal as AModal } from 'ant-design-vue'
import { computed, onMounted, ref } from 'vue'
import { messages } from '../messages'

import { useCredentialsStore } from '../stores/credentials'
import ErrorNotice from './ErrorNotice.vue'
import HelpText from './HelpText.vue'

defineProps<{ expanded?: boolean }>()
const credentials = useCredentialsStore()
const confirming = ref<string | null>(null)
const target = computed(() => credentials.snapshot?.enrollment ?? credentials.snapshot?.recovery)

function confirmRepair(): void {
  confirming.value = target.value?.fingerprint ?? null
}

function close(): void {
  if (!credentials.busy)
    confirming.value = null
}

async function repair(): Promise<void> {
  if (confirming.value === null || credentials.busy)
    return
  const fingerprint = confirming.value
  await credentials.repair(fingerprint)
  confirming.value = null
}

onMounted(() => {
  void credentials.refresh()
})
</script>
