<template>
  <section class="mx-auto max-w-[1088px] min-w-0 stack gap-5 pb-4" data-page-width="form">
    <PageHeader :title="messages.settings.title">
      <span v-if="hasDrafts" class="text-sm text-ink-dim" role="status">{{ messages.settings.edit.unsaved }}</span>
    </PageHeader>
    <div v-if="readError" class="stack gap-2">
      <SettingsFeedback :error="readError" />
      <AButton :loading="loading" @click="load">
        {{ messages.actions.retry }}
      </AButton>
    </div>
    <div class="grid min-w-0 items-start gap-6 lg:grid-cols-[max-content_minmax(0,1fr)] lg:gap-8">
      <nav :aria-label="messages.settings.navigation" class="min-w-0">
        <label for="settings-category" class="sr-only lg:hidden">{{ messages.settings.navigation }}</label>
        <ASelect id="settings-category" class="w-full lg:hidden" data-testid="settings-category" :value="category" :options="categories" @change="selectCategory" />
        <AMenu class="hidden overflow-hidden border border-line rounded-surface lg:block [&_.ant-menu]:bg-transparent" :aria-label="messages.settings.navigation" mode="inline" :selected-keys="[category]" @click="({ key }) => selectCategory(key)">
          <AMenu.Item v-for="item in categories" :key="item.value">
            {{ item.label }}{{ item.value === 'plugins' && pluginDrafts ? ' •' : '' }}
          </AMenu.Item>
        </AMenu>
      </nav>
      <div class="max-w-3xl min-w-0 w-full stack gap-6 border border-line rounded-surface bg-glass p-5">
        <h2 :ref="setCategoryHeading" tabindex="-1" class="sr-only m-0 text-base font-semibold">
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
                  <AButton v-if="nameConflict" :disabled="busy.name" @click="resetName">
                    {{ messages.settings.edit.loadCurrent }}
                  </AButton>
                  <AButton :disabled="busy.name || needsRead.name" @click="cancelRename">
                    {{ messages.settings.field.cancel }}
                  </AButton>
                </div>
                <span v-if="nameConflict" class="text-warning text-sm">{{ messages.settings.edit.conflict }}</span>
              </template>
              <div v-else-if="identity" class="min-w-0 row flex-wrap justify-end gap-2">
                <span class="min-w-0 break-all text-base">{{ identity.displayName }}</span>
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
            <SettingsRow :label="messages.settings.wire.show" :description="messages.settings.wire.detail">
              <ASwitch :checked="debugConsole" :aria-label="messages.settings.wire.show" :disabled="!configReady || busy.wire || needsRead.wire" :loading="busy.wire" @change="(value) => writeSetting('wire', { set: { 'ui.debugConsole': value === true } })" />
              <template #feedback>
                <SettingsFeedback :error="errors.wire" :saved="saved.wire" :check-required="needsRead.wire" :busy="busy.wire" @check="checkSetting('wire')" />
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
          <SettingsGroup :title="messages.settings.trusted.title">
            <p class="m-0 pb-3 text-sm text-ink-dim">
              {{ messages.settings.trusted.detail }}
            </p>
            <p v-if="!trusted.length" class="m-0 py-3 text-base text-ink-dim">
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
                  <label :for="`registry-${index}`" class="mb-1 block text-sm text-ink-dim">{{ messages.settings.registry.source(index + 1) }}</label>
                  <AInput :id="`registry-${index}`" :value="url" :disabled="!configReady || busy.registry || needsRead.registry" :placeholder="messages.settings.registry.placeholder" allow-clear @update:value="(value: string) => updateRegistry(index, value)" @press-enter="(event: KeyboardEvent) => { if (!event.isComposing) saveRegistry() }" />
                </div>
                <AButton v-if="registryUrls.length > 1" class="mt-5" :disabled="busy.registry || needsRead.registry" :aria-label="messages.settings.registry.removeSource(index + 1)" @click="registryUrls = registryUrls.filter((_, at) => at !== index)">
                  {{ messages.settings.registry.remove }}
                </AButton>
              </div>
              <HelpText>{{ messages.settings.registry.detail }}</HelpText>
              <span v-if="registryConflict" class="text-warning text-sm">{{ messages.settings.edit.conflict }}</span>
              <div class="row flex-wrap gap-2">
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
              <span v-if="retentionConflict" class="text-warning text-sm">{{ messages.settings.edit.conflict }}</span>
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
              <span class="text-base">{{ identity?.platform }}</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.field.deviceId">
              <span class="max-w-full break-all text-sm text-ink-dim font-mono">{{ identity?.deviceId }}</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.preference.theme">
              <span class="text-base text-ink-dim">{{ messages.settings.themeFixed }}</span>
            </SettingsRow>
          </SettingsGroup>
          <SettingsGroup :title="messages.settings.field.version">
            <SettingsRow :label="messages.settings.field.frontend">
              <span class="text-base font-mono">- v{{ buildStamp.version }}({{ buildStamp.shortHash || '—' }})</span>
            </SettingsRow>
            <SettingsRow :label="messages.settings.field.core">
              <span class="text-base font-mono" :title="coreBuildStamp.hash || undefined">- v{{ coreBuildStamp.version }}({{ coreBuildStamp.shortHash || '—' }})</span>
            </SettingsRow>
          </SettingsGroup>
        </div>
      </div>
    </div>
    <AModal :open="leaveVisible" :title="messages.settings.edit.leaveTitle" :closable="!anyBusy" :mask-closable="!anyBusy" :keyboard="!anyBusy" @cancel="resolveLeave(false)">
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
import { Button as AButton, Input as AInput, InputNumber as AInputNumber, Menu as AMenu, Modal as AModal, Select as ASelect, Switch as ASwitch } from 'ant-design-vue'
import { buildStamp, coreBuildStamp } from '../build-stamp'
import HelpText from '../components/HelpText.vue'
import PageHeader from '../components/PageHeader.vue'
import SettingsDiagnostics from '../components/settings/SettingsDiagnostics.vue'
import SettingsFeedback from '../components/settings/SettingsFeedback.vue'
import SettingsGroup from '../components/settings/SettingsGroup.vue'
import SettingsRow from '../components/settings/SettingsRow.vue'
import { localePreference } from '../locale'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useSettingsPage } from './settings/use-settings-page'

const { category, categories, setCategoryHeading, diagnosticsVisited, selectCategory, identity, readError, loading, renaming, startRename, cancelRename, checkName, saveName, askCancel, debugConsole, notifyOnReceive, languageOptions, updatePolicyOptions, updatePolicy, saveLanguage, saveUpdatePolicy, updateRegistry, saveRegistry, saveRetention, trusted, removingId, forgetTrusted, launchAtLogin, launchAtLoginUnavailable, saveLaunchAtLogin, pluginDrafts, hasDrafts, resolveLeave, saveAndLeave, load, errors, busy, saved, needsRead, anyBusy, configReady, checkSetting, writeSetting, nameDraft, nameDirty, nameConflict, registryUrls, registryDirty, registryConflict, retentionDays, retentionDirty, retentionConflict, resetName, resetRegistry, resetRetention, leaveOpen } = useSettingsPage()
const leaveVisible = useDialogVisibility(leaveOpen, 60)
</script>
