<template>
  <section class="min-w-0 stack gap-5 pb-4">
    <PageHeader :title="messages.settings.title">
      <span v-if="hasDrafts" class="text-xs text-ink-dim" role="status">{{ messages.settings.edit.unsaved }}</span>
    </PageHeader>
    <div v-if="readError" class="stack gap-2">
      <SettingsFeedback :error="readError" />
      <AButton :loading="loading" @click="load">
        {{ messages.actions.retry }}
      </AButton>
    </div>
    <AAlert v-if="identityNeedsAttention && category !== 'security'" type="warning" show-icon :message="messages.settings.credentials.attention">
      <template #action>
        <AButton size="small" @click="category = 'security'">
          {{ messages.settings.credentials.review }}
        </AButton>
      </template>
    </AAlert>
    <!--
      The category column is sized by its labels, not by a fixed 10rem: "Device and security" and "Plugins and sources"
      are wider than the Chinese labels, and a fixed track truncated them to "Device and sec…" in English while looking
      right in Chinese. `max-content` lets the longest label set the width; `minmax(0,1fr)` keeps the body able to shrink.
    -->
    <div class="grid min-w-0 items-start gap-6 lg:grid-cols-[max-content_minmax(0,1fr)] lg:gap-8">
      <nav :aria-label="messages.settings.navigation" class="min-w-0">
        <label for="settings-category" class="sr-only">{{ messages.settings.navigation }}</label>
        <ASelect id="settings-category" class="w-full lg:hidden" data-testid="settings-category" :value="category" :options="categories" @change="selectCategory" />
        <AMenu class="hidden lg:block" mode="inline" :selected-keys="[category]" @click="({ key }) => selectCategory(key)">
          <AMenu.Item v-for="item in categories" :key="item.value">
            {{ item.label }}{{ item.value === 'plugins' && pluginDrafts ? ' •' : '' }}
          </AMenu.Item>
        </AMenu>
      </nav>
      <div class="max-w-3xl min-w-0 w-full stack gap-6">
        <h2 ref="categoryHeading" tabindex="-1" class="m-0 text-base font-semibold outline-none">
          {{ categories.find(item => item.value === category)?.label }}
        </h2>

        <div v-show="category === 'general'" class="min-w-0 stack gap-7" data-testid="settings-general">
          <SettingsGroup :title="messages.settings.preferences">
            <SettingsRow :label="messages.settings.field.name" for-id="device-name">
              <template v-if="renaming">
                <AInput id="device-name" v-model:value="nameDraft" :disabled="busy.name || needsRead.name" @press-enter="(event: KeyboardEvent) => { if (!event.isComposing) saveName() }" />
                <div class="row flex-wrap gap-2">
                  <AButton type="primary" :loading="busy.name" :disabled="needsRead.name || !nameDirty || !nameDraft.trim()" @click="saveName">
                    {{ nameConflict ? messages.settings.edit.overwrite : messages.settings.field.save }}
                  </AButton>
                  <AButton :disabled="busy.name || needsRead.name" @click="cancelRename">
                    {{ messages.settings.field.cancel }}
                  </AButton>
                </div>
                <span v-if="nameConflict" class="text-warning text-xs">{{ messages.settings.edit.conflict }}</span>
              </template>
              <div v-else-if="identity" class="min-w-0 row flex-wrap justify-end gap-2">
                <span class="min-w-0 break-all text-sm">{{ identity.displayName }}</span>
                <AButton :disabled="busy.name" @click="startRename">
                  {{ messages.settings.field.rename }}
                </AButton>
              </div>
              <template #feedback>
                <SettingsFeedback :error="errors.name" :saved="saved.name" :check-required="needsRead.name" :busy="busy.name" @check="checkName" />
              </template>
            </SettingsRow>
            <SettingsRow :label="messages.settings.language.title" for-id="interface-language">
              <ASelect id="interface-language" data-testid="language-preference" class="w-full sm:w-52" :value="localePreference" :options="languageOptions" :loading="busy.language" :disabled="!configReady || busy.language || needsRead.language" @change="saveLanguage" />
              <template #feedback>
                <SettingsFeedback :error="errors.language" :saved="saved.language" :check-required="needsRead.language" :busy="busy.language" @check="checkSetting('language')" />
              </template>
            </SettingsRow>
            <SettingsRow :label="messages.settings.startup.launchAtLogin" :description="launchAtLoginUnavailable ? messages.settings.startup.unavailable : undefined">
              <ASwitch :checked="launchAtLogin" :aria-label="messages.settings.startup.launchAtLogin" :disabled="launchAtLoginUnavailable || busy.startup" :loading="busy.startup" @change="(value) => saveLaunchAtLogin(value === true)" />
              <template #feedback>
                <SettingsFeedback :error="errors.startup" :saved="saved.startup" />
              </template>
            </SettingsRow>
          </SettingsGroup>
          <SettingsGroup :title="messages.settings.transfers.title">
            <SettingsRow :label="messages.settings.transfers.cancelAsk">
              <ASwitch :checked="askCancel" :aria-label="messages.settings.transfers.cancelAsk" :disabled="!configReady || busy.cancel || needsRead.cancel" :loading="busy.cancel" @change="(value) => writeSetting('cancel', { set: { 'transfers.confirmCancel': value === true } })" />
              <template #feedback>
                <SettingsFeedback :error="errors.cancel" :saved="saved.cancel" :check-required="needsRead.cancel" :busy="busy.cancel" @check="checkSetting('cancel')" />
              </template>
            </SettingsRow>
            <SettingsRow :label="messages.settings.transfers.notify">
              <ASwitch :checked="notifyOnReceive" :aria-label="messages.settings.transfers.notify" :disabled="!configReady || busy.notify || needsRead.notify" :loading="busy.notify" @change="(value) => writeSetting('notify', { set: { 'transfers.notifyOnReceive': value === true } })" />
              <template #feedback>
                <SettingsFeedback :error="errors.notify" :saved="saved.notify" :check-required="needsRead.notify" :busy="busy.notify" @check="checkSetting('notify')" />
              </template>
            </SettingsRow>
          </SettingsGroup>
        </div>

        <div v-show="category === 'security'" class="min-w-0 stack gap-7" data-testid="settings-security">
          <CredentialPanel :expanded="category === 'security' && identityNeedsAttention" />
          <SettingsGroup :title="messages.settings.trusted.title">
            <p class="m-0 pb-3 text-xs text-ink-dim">
              {{ messages.settings.trusted.detail }}
            </p>
            <p v-if="!trusted.length" class="m-0 py-3 text-sm text-ink-dim">
              {{ messages.settings.trusted.empty }}
            </p>
            <SettingsRow v-for="device in trusted" :key="device.id" :label="device.name">
              <AButton :disabled="!configReady || busy.trusted || needsRead.trusted" :loading="busy.trusted && removingId === device.id" @click="forgetTrusted(device.id)">
                {{ messages.settings.trusted.remove }}
              </AButton>
            </SettingsRow>
            <SettingsFeedback :error="errors.trusted" :saved="saved.trusted" :check-required="needsRead.trusted" :busy="busy.trusted" @check="checkSetting('trusted')" />
          </SettingsGroup>
        </div>

        <div v-show="category === 'plugins'" class="min-w-0 stack gap-7" data-testid="settings-plugins">
          <SettingsGroup :title="messages.settings.registry.title">
            <div class="min-w-0 stack gap-3 pb-4">
              <div v-for="(url, index) in registryUrls" :key="index" class="min-w-0 row items-start gap-2">
                <div class="min-w-0 flex-1">
                  <label :for="`registry-${index}`" class="mb-1 block text-xs text-ink-dim">{{ messages.settings.registry.source(index + 1) }}</label>
                  <AInput :id="`registry-${index}`" :value="url" :disabled="!configReady || busy.registry || needsRead.registry" :placeholder="messages.settings.registry.placeholder" allow-clear @update:value="(value: string) => updateRegistry(index, value)" @press-enter="(event: KeyboardEvent) => { if (!event.isComposing) saveRegistry() }" />
                </div>
                <AButton v-if="registryUrls.length > 1" class="mt-5" :disabled="busy.registry || needsRead.registry" :aria-label="messages.settings.registry.removeSource(index + 1)" @click="registryUrls = registryUrls.filter((_, at) => at !== index)">
                  {{ messages.settings.registry.remove }}
                </AButton>
              </div>
              <HelpText>{{ messages.settings.registry.detail }}</HelpText>
              <span v-if="registryConflict" class="text-warning text-xs">{{ messages.settings.edit.conflict }}</span>
              <div class="row flex-wrap gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
                <AButton :disabled="!configReady || busy.registry || needsRead.registry" @click="registryUrls = [...registryUrls, '']">
                  {{ messages.settings.registry.add }}
                </AButton>
                <AButton type="primary" data-testid="save-registry" :loading="busy.registry" :disabled="!configReady || !registryDirty || needsRead.registry" @click="saveRegistry">
                  {{ registryConflict ? messages.settings.edit.overwrite : messages.settings.field.save }}
                </AButton>
                <AButton v-if="registryDirty" :disabled="busy.registry || needsRead.registry" @click="resetRegistry">
                  {{ registryConflict ? messages.settings.edit.loadCurrent : messages.settings.field.cancel }}
                </AButton>
              </div>
              <SettingsFeedback :error="errors.registry" :saved="saved.registry" :check-required="needsRead.registry" :busy="busy.registry" @check="checkSetting('registry')" />
            </div>
          </SettingsGroup>
          <SettingsGroup :title="messages.settings.updates.title">
            <SettingsRow :label="messages.settings.updates.policy" for-id="plugin-update-policy" :description="messages.settings.updates.detail">
              <ASelect id="plugin-update-policy" data-testid="plugin-update-policy" class="w-full" :value="updatePolicy" :loading="busy.policy" :disabled="!configReady || busy.policy || needsRead.policy" :options="updatePolicyOptions" @change="saveUpdatePolicy" />
              <template #feedback>
                <SettingsFeedback :error="errors.policy" :saved="saved.policy" :check-required="needsRead.policy" :busy="busy.policy" @check="checkSetting('policy')" />
              </template>
            </SettingsRow>
          </SettingsGroup>
          <SettingsGroup :title="messages.settings.audit.title">
            <SettingsRow :label="messages.settings.audit.days" for-id="retention-days" :description="messages.settings.audit.detail">
              <div class="row flex-wrap gap-2">
                <AInputNumber id="retention-days" :value="retentionDays" :disabled="!configReady || busy.retention || needsRead.retention" :min="1" :max="3650" :precision="0" @update:value="(value) => retentionDays = typeof value === 'number' ? value : 1" />
                <AButton :loading="busy.retention" :disabled="!configReady || !retentionDirty || needsRead.retention" @click="saveRetention">
                  {{ retentionConflict ? messages.settings.edit.overwrite : messages.settings.field.save }}
                </AButton>
                <AButton v-if="retentionDirty" :disabled="busy.retention || needsRead.retention" @click="resetRetention">
                  {{ retentionConflict ? messages.settings.edit.loadCurrent : messages.settings.field.cancel }}
                </AButton>
              </div>
              <span v-if="retentionConflict" class="text-warning text-xs">{{ messages.settings.edit.conflict }}</span>
              <template #feedback>
                <SettingsFeedback :error="errors.retention" :saved="saved.retention" :check-required="needsRead.retention" :busy="busy.retention" @check="checkSetting('retention')" />
              </template>
            </SettingsRow>
          </SettingsGroup>
        </div>

        <SettingsDiagnostics v-if="diagnosticsVisited" v-show="category === 'diagnostics'" :active="category === 'diagnostics'" />

        <div v-show="category === 'about'" class="min-w-0 stack gap-7" data-testid="settings-about">
          <SettingsGroup title="Synra">
            <SettingsRow :label="messages.settings.field.platform">
              <span class="text-sm">{{ identity?.platform }}</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.field.kernel">
              <span v-if="identity" class="text-sm font-mono">v{{ identity.kernelVersion }}</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.field.deviceId">
              <span class="max-w-full break-all text-xs text-ink-dim font-mono">{{ identity?.deviceId }}</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.preference.theme">
              <span class="text-sm text-ink-dim">{{ messages.settings.themeFixed }}</span>
            </SettingsRow>
          </SettingsGroup>
        </div>
      </div>
    </div>
    <AModal :open="leaveOpen" :title="messages.settings.edit.leaveTitle" :closable="!anyBusy" :mask-closable="!anyBusy" :keyboard="!anyBusy" @cancel="resolveLeave(false)">
      <p>{{ messages.settings.edit.leaveDetail }}</p>
      <template #footer>
        <AButton :disabled="anyBusy" @click="resolveLeave(false)">
          {{ messages.settings.edit.stay }}
        </AButton>
        <AButton :disabled="anyBusy" @click="resolveLeave(true)">
          {{ messages.settings.edit.discard }}
        </AButton>
        <AButton type="primary" :loading="anyBusy" @click="saveAndLeave">
          {{ messages.settings.edit.saveAll }}
        </AButton>
      </template>
    </AModal>
  </section>
</template>

<script setup lang="ts">
import type { ConfigPatch, IpcError } from '@synra/protocol'
import { Alert as AAlert, Button as AButton, Input as AInput, InputNumber as AInputNumber, Menu as AMenu, Modal as AModal, Select as ASelect, Switch as ASwitch } from 'ant-design-vue'
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import CredentialPanel from '../components/CredentialPanel.vue'
import HelpText from '../components/HelpText.vue'
import PageHeader from '../components/PageHeader.vue'
import SettingsDiagnostics from '../components/settings/SettingsDiagnostics.vue'
import SettingsFeedback from '../components/settings/SettingsFeedback.vue'
import SettingsGroup from '../components/settings/SettingsGroup.vue'
import SettingsRow from '../components/settings/SettingsRow.vue'
import { useSettingsDraft } from '../components/settings/use-settings-draft'
import { kernel } from '../kernel'
import { localePreference } from '../locale'
import { messages } from '../messages'
import { useCredentialsStore } from '../stores/credentials'
import { useDevicesStore } from '../stores/devices'
import { useSettingsStore } from '../stores/settings'

type Category = 'general' | 'security' | 'plugins' | 'diagnostics' | 'about'
type Operation = 'name' | 'language' | 'startup' | 'cancel' | 'notify' | 'trusted' | 'registry' | 'policy' | 'retention'
const category = ref<Category>('general')
const categories = computed(() => (['general', 'security', 'plugins', 'diagnostics', 'about'] as const).map(value => ({ value, label: messages.settings.categories[value] })))
const categoryHeading = ref<HTMLElement>()
const diagnosticsVisited = ref(false)
function selectCategory(value: unknown): void {
  if (categories.value.some(item => item.value === value))
    category.value = value as Category
}
watch(category, async (value) => {
  if (value === 'diagnostics')
    diagnosticsVisited.value = true
  await nextTick()
  categoryHeading.value?.focus({ preventScroll: true })
})

const settings = useSettingsStore()
const devices = useDevicesStore()
const credentials = useCredentialsStore()
const identity = computed(() => devices.identity)
const identityNeedsAttention = computed(() => Boolean(credentials.error || credentials.restartRequired || credentials.snapshot?.recovery
  || (credentials.snapshot && (!credentials.snapshot.runtimeReady || !credentials.snapshot.enrollment?.ready || credentials.snapshot.retiredKeys > 0))))
const errors = reactive<Partial<Record<Operation, IpcError | undefined>>>({})
const operationFlags = (): Record<Operation, boolean> => ({ name: false, language: false, startup: false, cancel: false, notify: false, trusted: false, registry: false, policy: false, retention: false })
const busy = reactive(operationFlags())
const saved = reactive(operationFlags())
// A successful write followed by a failed read is not an invitation to send the write again.
const needsRead = reactive(operationFlags())
const anyBusy = computed(() => Object.values(busy).some(Boolean))
const configReady = ref(false)
const readError = ref<IpcError>()
const loading = ref(false)
let disposed = false

const { value: nameDraft, dirty: nameDirty, conflict: nameConflict, reset: resetName } = useSettingsDraft(() => identity.value?.displayName ?? '')
const renaming = ref(false)
function startRename(): void {
  resetName()
  renaming.value = true
  errors.name = undefined
  saved.name = false
  void nextTick(() => document.getElementById('device-name')?.focus())
}
function cancelRename(): void {
  resetName()
  renaming.value = false
  errors.name = undefined
}
async function checkName(): Promise<boolean> {
  const answer = await devices.readIdentity(kernel)
  if (disposed)
    return false
  if (!answer.ok) {
    errors.name = answer.error
    return false
  }
  needsRead.name = false
  errors.name = undefined
  saved.name = true
  resetName()
  renaming.value = false
  return true
}
async function saveName(): Promise<boolean> {
  if (busy.name)
    return false
  busy.name = true
  errors.name = undefined
  try {
    if (needsRead.name)
      return await checkName()
    if (!nameDirty.value || !nameDraft.value.trim())
      return !nameDirty.value
    const answer = await kernel.call<null>('identity_set_display_name', { name: nameDraft.value })
    if (disposed)
      return false
    if (!answer.ok) {
      errors.name = answer.error
      return false
    }
    needsRead.name = true
    return await checkName()
  }
  finally {
    busy.name = false
  }
}

const askCancel = computed(() => settings.value('transfers.confirmCancel') !== false)
const notifyOnReceive = computed(() => settings.value('transfers.notifyOnReceive') !== false)
const languageOptions = computed(() => [
  { value: 'system', label: messages.settings.language.system },
  { value: 'zh-CN', label: messages.settings.language.chinese },
  { value: 'en', label: messages.settings.language.english }
])
const updatePolicyOptions = computed(() => [
  { value: 'never', label: messages.settings.updates.never },
  { value: 'notify-only', label: messages.settings.updates.notifyOnly },
  { value: 'auto-minor', label: messages.settings.updates.autoMinor },
  { value: 'auto-all', label: messages.settings.updates.autoAll }
])
const updatePolicy = computed(() => {
  const value = settings.value('plugins.auto_update')
  return typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value) ? value : 'notify-only'
})
async function saveLanguage(value: unknown): Promise<void> {
  if (value === 'system' || value === 'en' || value === 'zh-CN')
    await writeSetting('language', { set: { 'ui.locale': value } })
}
async function saveUpdatePolicy(value: unknown): Promise<void> {
  if (typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value))
    await writeSetting('policy', { set: { 'plugins.auto_update': value } })
}

function registrySource(): string[] {
  const raw = settings.value('plugins.registry')
  const urls = typeof raw === 'string' ? [raw] : Array.isArray(raw) ? raw.filter((item): item is string => typeof item === 'string') : []
  return urls.length ? urls : ['']
}
const { value: registryUrls, dirty: registryDirty, conflict: registryConflict, reset: resetRegistry } = useSettingsDraft(registrySource)
const { value: retentionDays, dirty: retentionDirty, conflict: retentionConflict, reset: resetRetention } = useSettingsDraft(() => {
  const raw = settings.value('audit.retentionDays')
  return typeof raw === 'number' && Number.isFinite(raw) ? raw : 7
})
function updateRegistry(index: number, value: string): void {
  registryUrls.value = registryUrls.value.map((url, at) => index === at ? value : url)
  saved.registry = false
}
async function saveRegistry(): Promise<boolean> {
  if (!registryDirty.value && !needsRead.registry)
    return true
  const urls = registryUrls.value.map(url => url.trim()).filter(Boolean)
  return await writeSetting('registry', urls.length ? { set: { 'plugins.registry': urls.length === 1 ? urls[0]! : urls } } : { unset: ['plugins.registry'] })
}
async function saveRetention(): Promise<boolean> {
  if (!retentionDirty.value && !needsRead.retention)
    return true
  return await writeSetting('retention', { set: { 'audit.retentionDays': Math.min(3650, Math.max(1, Math.trunc(retentionDays.value))) } })
}
const trusted = computed(() => {
  const raw = settings.value('transfers.acceptFrom')
  return (Array.isArray(raw) ? raw.filter((id): id is string => typeof id === 'string') : []).map(id => ({ id, name: devices.get(id)?.displayName ?? id }))
})
const removingId = ref<string>()
async function forgetTrusted(id: string): Promise<void> {
  if (busy.trusted || needsRead.trusted)
    return
  removingId.value = id
  const remaining = trusted.value.filter(device => device.id !== id).map(device => device.id)
  await writeSetting('trusted', remaining.length ? { set: { 'transfers.acceptFrom': remaining } } : { unset: ['transfers.acceptFrom'] })
}

async function readSetting(operation: Operation): Promise<boolean> {
  const answer = await settings.read(kernel)
  if (disposed)
    return false
  if (!answer.ok) {
    errors[operation] = answer.error
    return false
  }
  configReady.value = true
  needsRead[operation] = false
  errors[operation] = undefined
  saved[operation] = true
  if (operation === 'registry')
    resetRegistry()
  if (operation === 'retention')
    resetRetention()
  return true
}
async function checkSetting(operation: Operation): Promise<boolean> {
  if (busy[operation])
    return false
  busy[operation] = true
  try {
    return await readSetting(operation)
  }
  finally {
    busy[operation] = false
  }
}
async function writeSetting(operation: Operation, patch: ConfigPatch): Promise<boolean> {
  if (busy[operation] || !configReady.value)
    return false
  busy[operation] = true
  saved[operation] = false
  errors[operation] = undefined
  try {
    if (needsRead[operation])
      return await readSetting(operation)
    const answer = await kernel.call<null>('config_set', { patch })
    if (disposed)
      return false
    if (!answer.ok) {
      errors[operation] = answer.error
      return false
    }
    needsRead[operation] = true
    return await readSetting(operation)
  }
  finally {
    busy[operation] = false
  }
}

const launchAtLogin = ref(false)
const launchAtLoginUnavailable = ref(false)
async function loadLaunchAtLogin(): Promise<void> {
  try {
    const { isEnabled } = await import('@tauri-apps/plugin-autostart')
    launchAtLogin.value = await isEnabled()
  }
  catch {
    launchAtLoginUnavailable.value = true
  }
}
async function saveLaunchAtLogin(value: boolean): Promise<void> {
  if (busy.startup || launchAtLoginUnavailable.value)
    return
  busy.startup = true
  errors.startup = undefined
  saved.startup = false
  try {
    const { enable, disable, isEnabled } = await import('@tauri-apps/plugin-autostart')
    if (value)
      await enable()
    else
      await disable()
    launchAtLogin.value = await isEnabled()
    saved.startup = true
  }
  catch {
    errors.startup = { code: 'internal', message: 'error.internal', retryable: false }
  }
  finally {
    busy.startup = false
  }
}

const pluginDrafts = computed(() => registryDirty.value || retentionDirty.value || needsRead.registry || needsRead.retention)
const hasDrafts = computed(() => (renaming.value && (nameDirty.value || needsRead.name)) || pluginDrafts.value)
const leaveOpen = ref(false)
let leaveResolve: ((leave: boolean) => void) | undefined
function resolveLeave(leave: boolean): void {
  leaveOpen.value = false
  leaveResolve?.(leave)
  leaveResolve = undefined
}
onBeforeRouteLeave(() => {
  if (!hasDrafts.value && !anyBusy.value)
    return true
  if (leaveResolve)
    return false
  leaveOpen.value = true
  return new Promise<boolean>((resolve) => {
    leaveResolve = resolve
  })
})
async function saveAndLeave(): Promise<void> {
  if (anyBusy.value)
    return
  if (nameConflict.value || registryConflict.value || retentionConflict.value) {
    category.value = nameConflict.value ? 'general' : 'plugins'
    resolveLeave(false)
    return
  }
  if (renaming.value && !await saveName()) {
    category.value = 'general'
    resolveLeave(false)
    return
  }
  if (!await saveRegistry() || !await saveRetention()) {
    category.value = 'plugins'
    resolveLeave(false)
    return
  }
  resolveLeave(true)
}
function beforeUnload(event: BeforeUnloadEvent): void {
  if (hasDrafts.value || anyBusy.value) {
    event.preventDefault()
    event.returnValue = ''
  }
}
async function load(): Promise<void> {
  if (loading.value)
    return
  loading.value = true
  const [config, name] = await Promise.all([settings.read(kernel), devices.readIdentity(kernel)])
  if (!disposed) {
    if (config.ok)
      configReady.value = true
    readError.value = !config.ok ? config.error : !name.ok ? name.error : undefined
  }
  loading.value = false
}
onMounted(() => {
  void load()
  void loadLaunchAtLogin()
  window.addEventListener('beforeunload', beforeUnload)
})
onUnmounted(() => {
  disposed = true
  resolveLeave(false)
  window.removeEventListener('beforeunload', beforeUnload)
})
</script>
