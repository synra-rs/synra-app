<template>
  <section class="stack gap-6">
    <h1 class="text-xl font-semibold">
      {{ messages.plugins.title }}
    </h1>

    <!--
      The install form. Two sources, named rather than guessed: a directory on this device, or a package id in the
      registry configured on the settings page. The mode is a control instead of a rule that inspects the text, because
      `@acme/hello` and `C:\plugins\hello` are both "a string with a slash" and a page that guessed would eventually guess
      wrong.
    -->
    <ASegmented v-model:value="installMode" :options="installOptions" />
    <div class="row gap-2">
      <AInput
        v-model:value="installPath"
        :placeholder="installMode === 'registry' ? messages.plugins.installName : messages.plugins.installPath"
        allow-clear
        @press-enter="install"
      />
      <AButton :loading="installing" :disabled="installPath.trim() === ''" @click="install">
        {{ installing ? messages.plugins.installing : messages.plugins.installAction }}
      </AButton>
    </div>

    <!--
      The registry's own index. A result is a name and a description, not a decision: Preview opens the same permission
      dialog an install by id would, so a search hit cannot install anything the user has not seen.
    -->
    <section class="stack gap-2">
      <div class="row gap-2">
        <AInput
          v-model:value="searchText"
          :placeholder="messages.plugins.catalog.placeholder"
          allow-clear
          @press-enter="search"
        />
        <ASelect v-model:value="searchSort" :options="sortOptions" class="w-fit" />
        <AButton :loading="searching" :disabled="searchText.trim() === ''" @click="search">
          {{ messages.plugins.catalog.action }}
        </AButton>
      </div>
      <AList v-if="results.length > 0" :data-source="results" size="small">
        <template #renderItem="{ item }">
          <AList.Item>
            <div class="row-between gap-4">
              <div class="stack">
                <span class="truncate text-sm font-medium">
                  {{ item.title }} <span class="text-ink-dim">{{ item.version }}</span>
                </span>
                <span class="text-sm text-ink-dim">{{ item.description ?? item.pluginId }}</span>
              </div>
              <AButton size="small" @click="previewResult(item)">
                {{ messages.plugins.catalog.preview }}
              </AButton>
            </div>
          </AList.Item>
        </template>
      </AList>
      <AButton v-if="hasMore" :loading="searching" block @click="loadMore">
        {{ messages.plugins.catalog.loadMore }}
      </AButton>
      <p v-else-if="searched && !searching" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.catalog.empty }}
      </p>
    </section>

    <!--
      A registry install asks first. The preview is the publisher's own claim — the signature is checked on the bytes —
      and it is a question, not an approval: the plugin still lands switched off and untrusted.
    -->
    <AModal
      :open="preview !== undefined"
      :title="messages.plugins.preview.title"
      :ok-text="messages.plugins.installAction"
      @ok="confirmPreview"
      @cancel="preview = undefined"
    >
      <div v-if="preview" class="stack gap-2">
        <p class="m-0 text-base font-medium">
          {{ preview.title }} · {{ preview.version }}
        </p>
        <p class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.publisher }}: {{ preview.publisherName ?? preview.publisher }}
        </p>
        <p v-if="preview.publisherKey" class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.directoryKey }}: <span class="font-mono">{{ preview.publisherKey }}</span>
        </p>
        <p class="m-0 text-sm">
          {{ messages.plugins.preview.permissions }}
        </p>
        <ul v-if="preview.capabilities.length > 0" class="m-0 ps-5 text-sm">
          <li v-for="capability in preview.capabilities" :key="capability" class="font-mono">
            {{ capability }}
          </li>
        </ul>
        <p v-else class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.none }}
        </p>
        <p class="m-0 text-sm text-ink-dim">
          {{ preview.headless ? messages.plugins.preview.headless : messages.plugins.preview.ui }}
        </p>
        <p v-if="preview.sizeBytes" class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.size.replace('{size}', (preview.sizeBytes / 1_048_576).toFixed(1)) }}
        </p>
      </div>
    </AModal>

    <!--
      ★ The removal dialog, not a one-line confirm, because there are two outcomes and one of them deletes user data.
      `get-container="false"` keeps it inside this component instead of teleporting to `body`: the page's own tests can
      then see the checkbox and the button, and a dialog that is testable is one whose decision is actually asserted.

      The checkbox starts unticked every time the dialog opens: "keep the data" is the reversible answer, and a checkbox
      that remembered the previous destructive choice would be a dialog that deletes on a second visit by default.
    -->
    <AModal
      :open="removing !== undefined"
      :title="messages.plugins.removeTitle"
      :ok-text="messages.plugins.removeAction"
      :get-container="false"
      @ok="remove()"
      @cancel="removing = undefined"
    >
      <div class="stack gap-3">
        <p class="m-0 text-sm">
          {{ messages.plugins.removeDetail(removing?.title ?? '') }}
        </p>
        <span class="break-all text-xs text-ink-dim font-mono">
          {{ messages.plugins.removeDataPath(removing?.dataPath ?? '') }}
        </span>
        <ACheckbox
          :checked="removeData"
          @update:checked="(checked) => (removeData = checked)"
        >
          {{ messages.plugins.removeDeleteData }}
        </ACheckbox>
      </div>
    </AModal>

    <!-- Installing is not approving: the sentence says the plugin is off, because a user told only "installed" would
         look for a fault in a plugin that is behaving exactly as designed. -->
    <AAlert v-if="note" type="success" :message="note" closable @close="note = undefined" />

    <!--
      The question a signed package asks next, shown right after the install that produced it. It is deliberately not a
      modal: the plugin is off, nothing is running, and the fingerprint is something a person compares at their own pace.
    -->
    <AAlert v-if="pendingPublisher" type="info" show-icon :message="messages.plugins.publisherPrompt.title">
      <template #description>
        <div class="stack gap-2">
          <span class="text-sm">{{ messages.plugins.publisherPrompt.detail }}</span>
          <span class="truncate text-xs font-mono">{{ pendingPublisher.fingerprint }}</span>
          <span class="row gap-2">
            <AButton
              size="small"
              :loading="deciding === pendingPublisher.id"
              @click="trustPendingPublisher"
            >
              {{ messages.plugins.publisherPrompt.trust }}
            </AButton>
            <AButton
              size="small"
              :loading="deciding === pendingPublisher.id"
              @click="revokePendingPublisher"
            >
              {{ messages.plugins.publisherPrompt.revoke }}
            </AButton>
            <AButton size="small" @click="pendingPublisher = undefined">
              {{ messages.plugins.publisherPrompt.later }}
            </AButton>
          </span>
        </div>
      </template>
    </AAlert>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="load" />

    <!--
      R28: an empty state is an invitation with a next step. The next step is now the **form above** — this screen can
      install — so the button here is the honest second option: read the list again, for a plugin installed from
      somewhere else. It said "this screen cannot install anything yet" while that was true; leaving the sentence behind
      would be the kind of comment that outlives its fact and sends the next reader looking for a missing feature.
    -->
    <!--
      A filter over what is installed, not a catalogue search: this shell has no registry client, and a box that looked
      like one would promise results it cannot fetch (16 §4). It matches the id, the title and the description, because
      those are the three things a person remembers about a plugin.
    -->
    <AInput
      v-if="plugins.length > 0"
      v-model:value="query"
      :placeholder="messages.plugins.filterPlaceholder"
      allow-clear
    />

    <AEmpty v-if="plugins?.length === 0" :description="messages.plugins.empty">
      <template #image>
        <!--
          The icon rather than antd's default illustration: that illustration draws its own light-grey palette into a
          screen whose neutrals are all one hue (§1.4), and empty states are named as the places that must not compete
          for attention (R27).
        -->
        <span class="i-carbon-application text-xl text-ink-dim" aria-hidden="true" />
      </template>

      <AButton :loading="loading" @click="load">
        {{ messages.plugins.emptyAction }}
      </AButton>
    </AEmpty>

    <AEmpty v-else-if="visible.length === 0" :description="messages.plugins.noMatch" />

    <!--
      A plugin is a substantial object — a name, three machine facts, a description, two permission lists and possibly
      an update — so each one is a list item with several lines rather than a card (R26). The lines are labelled
      separately instead of joined by separators, which is the shape R21 retires.
    -->
    <AList v-else :data-source="visible">
      <template #renderItem="{ item: plugin }">
        <AList.Item>
          <div class="w-full stack gap-3">
            <div class="row-between gap-4">
              <span class="truncate text-base font-medium">{{ plugin.title }}</span>

              <!-- R23: the state is the word in the tag; the colour is the redundancy that survives a greyscale shot. -->
              <!-- The switch is the user's decision about whether it runs; should_run is this **and** the consent. -->
              <ASwitch
                :checked="plugin.enabled"
                :loading="toggling === plugin.pluginId"
                @change="(checked) => toggle(plugin, checked as boolean)"
              />
              <ATag :color="stateColour(plugin.state)" class="shrink-0">
                {{ plugin.state }}
              </ATag>
            </div>

            <!--
              ★ The rung, when it is not `good`. A plugin that is over its limits is still `enabled` and may even be
              `running`; the state tag does not tell that story, so the health line does — and it is the sentence the
              quota itself produced, not a second opinion written here.
            -->
            <AAlert
              v-if="plugin.health && plugin.health.health !== 'good'"
              :type="plugin.health.health === 'faulted' ? 'error' : 'warning'"
              :message="messages.plugins.healthLabel(plugin.health.health)"
              :description="plugin.health.explanation"
              show-icon
            />

            <dl class="m-0 row flex-wrap gap-4 text-xs">
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.pluginId }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.pluginId }}
                </dd>
              </div>
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.version }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.version }}
                </dd>
              </div>
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.publisher }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.publisher }}
                </dd>
              </div>
              <!-- ★ Four of the quota's numbers, when the report carries a quota. The rung and the sentence say how the
                   plugin is doing; these say what it is allowed to do, which is the other half of the same question. -->
              <div v-if="healthLimits(plugin.health)" class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.limits }}
                </dt>
                <dd class="m-0">
                  {{ healthLimits(plugin.health) }}
                </dd>
              </div>
            </dl>

            <p v-if="plugin.description" class="m-0 text-sm text-ink-dim">
              {{ plugin.description }}
            </p>

            <!--
              What the registry publishes, when the user asked. It is not fetched on mount: one check is one request per
              plugin, and a list of ten would make opening the page depend on ten round trips.
            -->
            <AAlert
              v-if="updates[plugin.pluginId]"
              type="info"
              show-icon
              :message="messages.plugins.updateFound.replace('{version}', updates[plugin.pluginId]?.version ?? '')"
            >
              <template #description>
                <span v-if="(updates[plugin.pluginId]?.addedCapabilities ?? []).length === 0">
                  {{ messages.plugins.updateNoNewPermissions }}
                </span>
                <span v-else>
                  {{ messages.plugins.updateNewPermissions }} {{ updates[plugin.pluginId]?.addedCapabilities.join(', ') }}
                </span>
              </template>
            </AAlert>

            <!--
              What the plugin asked for, and what it was refused: the second list is the one that turns this screen from
              a settings page into the thing a user reads before installing something. "Nothing" is a word rather than
              an empty space, because an empty space reads as "not loaded".
            -->
            <div class="stack gap-1">
              <div class="row flex-wrap gap-2">
                <span class="text-xs text-ink-dim">{{ messages.plugins.granted }}</span>
                <span v-if="plugin.granted?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                <ATag v-for="capability in plugin.granted" :key="capability" class="font-mono">
                  {{ capability }}
                </ATag>
              </div>

              <div class="row flex-wrap gap-2">
                <span class="text-xs text-ink-dim">{{ messages.plugins.refused }}</span>
                <span v-if="plugin.denied?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                <ATag v-for="capability in plugin.denied" :key="capability" color="error" class="font-mono">
                  {{ capability }}
                </ATag>
              </div>
            </div>

            <!--
              The decision itself: one checkbox per **requested** capability, starting from what was already granted.

              ★ Not an "allow everything" button. This is the one screen where the user is asked what a plugin may do,
              and a single button would hand over exactly what the per-capability model exists to itemise. `requested`
              is what the manifest declares, so the boxes are what the *plugin* asks for rather than what it was given.
            -->
            <div v-if="plugin.requested?.length" class="stack gap-2">
              <span class="text-xs text-ink-dim">{{ messages.plugins.requested }}</span>

              <!--
                `:value` + `@update:value` rather than `v-model`: the map may not have an entry yet on the first render,
                and `v-model` on a possibly-undefined key does not type-check under `exactOptionalPropertyTypes`.
              -->
              <ACheckboxGroup
                :value="selection[plugin.pluginId] ?? []"
                class="stack"
                @update:value="(picked) => (selection[plugin.pluginId] = picked as string[])"
              >
                <ACheckbox
                  v-for="capability in plugin.requested"
                  :key="capability"
                  :value="capability"
                  class="text-xs font-mono"
                >
                  {{ capability }}
                </ACheckbox>
              </ACheckboxGroup>

              <AButton
                size="small"
                :loading="consenting === plugin.pluginId"
                @click="consent(plugin, selection[plugin.pluginId] ?? [])"
              >
                {{ messages.plugins.consentAction }}
              </AButton>
            </div>

            <!--
              ★ What the plugin **actually did**, beside what it may do. The granted list is a permission; this is a record
              of the gate's calls, which is what makes "granted" different from "used" (09 §159, 18 §5).

              Reading is a separate question from granting, so the trail is *pulled* rather than pushed: a page that asked on
              mount would read one ring per plugin that nobody asked about, and a page that read once would show activity from
              before whatever the user came back to check.
            -->
            <details class="stack gap-2" @toggle="(event) => openAudit(plugin.pluginId, event)">
              <summary class="cursor-pointer text-xs text-ink-dim">
                {{ messages.plugins.audit.title }}
              </summary>
              <ul v-if="(audit[plugin.pluginId]?.length ?? 0) > 0" class="m-0 ps-5 text-xs">
                <!--
                  The index is part of the key: two calls to the same capability can happen in the same millisecond, and the
                  ring records both. Keying on the pair would make Vue draw one row for two calls.
                -->
                <li v-for="(entry, index) in audit[plugin.pluginId]" :key="`${entry.atMs}-${entry.capability}-${index}`">
                  <span class="font-mono">{{ entry.capability }}</span>
                  <span :class="entry.allowed ? 'text-ink-dim' : 'text-danger'">
                    {{ entry.allowed ? messages.plugins.audit.allowed : messages.plugins.audit.refused }}
                  </span>
                  <!-- Why it was refused, when the gate said why: the stable outcome name, not a sentence this page wrote. -->
                  <span v-if="!entry.allowed && entry.reason" class="text-xs text-ink-dim font-mono">{{ entry.reason }}</span>
                  <!-- ★ The action this call belongs to. It is the id a bug report quotes for `synra logs --trace`. -->
                  <span v-if="entry.traceId" class="text-xs text-ink-dim font-mono" :title="entry.traceId">
                    {{ messages.plugins.audit.trace }} {{ entry.traceId }}
                  </span>
                  <span class="text-ink-dim">{{ new Date(entry.atMs).toLocaleTimeString() }}</span>
                </li>
              </ul>
              <p v-else-if="audit[plugin.pluginId] !== undefined" class="m-0 text-xs text-ink-dim">
                {{ messages.plugins.audit.empty }}
              </p>
              <!--
                ★ Only beside rows that exist. Clearing an empty trail would be a button whose only effect is to redraw
                the sentence above it, and the absence of the button is also the honest state of "nothing to clear".

                No confirmation. This is a log of what a plugin did, not data the plugin produced; the button is inside a
                section the user chose to open, and a dialog for it would be the friction that teaches people to dismiss
                dialogs. The irreversible-action rule in this page is about removal, which deletes files.
              -->
              <AButton
                v-if="(audit[plugin.pluginId]?.length ?? 0) > 0"
                size="small"
                :loading="clearingAudit === plugin.pluginId"
                @click="clearTrail(plugin.pluginId)"
              >
                {{ messages.plugins.audit.clear }}
              </AButton>
              <!--
                ★ Offered only while the last page came back **full** — that is the one signal a limit-only query gives
                that more rows may exist. Clicking it when nothing remains answers `[]` and hides the button, which is one
                wasted read rather than a control that disappears while a page was still coming.
              -->
              <AButton
                v-if="!auditDone[plugin.pluginId] && (audit[plugin.pluginId]?.length ?? 0) > 0"
                size="small"
                :loading="loadingMoreAudit === plugin.pluginId"
                @click="loadMoreAudit(plugin.pluginId)"
              >
                {{ messages.plugins.audit.more }}
              </AButton>
            </details>

            <!--
              Removal asks first, and the question says what happens to the data: this is the one action on the card that
              cannot be undone, and a button that only said "Remove" would leave the user to find out what it took with it.
            -->
            <div class="row gap-2">
              <!--
                ★ Only when the card says the plugin has a page. `hasUi`, not `hybrid`: a UI-only plugin has a page and no
                headless half, and a card that gated on both would leave that shape with no way in.
              -->
              <AButton v-if="plugin.hasUi" size="small" :loading="opening === plugin.pluginId" @click="open(plugin)">
                {{ messages.plugins.openAction }}
              </AButton>

              <AButton size="small" :loading="syncing === plugin.pluginId" @click="sync(plugin)">
                {{ syncing === plugin.pluginId ? messages.plugins.syncing : messages.plugins.syncAction }}
              </AButton>

              <AButton size="small" :loading="checking === plugin.pluginId" @click="checkUpdate(plugin)">
                {{ messages.plugins.checkUpdates }}
              </AButton>

              <AButton
                v-if="updates[plugin.pluginId]"
                type="primary"
                size="small"
                :loading="updating === plugin.pluginId"
                @click="applyUpdate(plugin)"
              >
                {{ messages.plugins.updateAction }}
              </AButton>

              <AButton size="small" danger @click="askRemove(plugin)">
                {{ messages.plugins.removeAction }}
              </AButton>
            </div>

            <!--
              An update is a decision rather than routine news when it asks for something new, so it is an alert with
              the version and the added permissions in it — and the permissions are named, not counted (R33's spirit:
              the user is told what they are being asked to allow).
            -->
            <AAlert v-if="plugin.update" type="warning" show-icon :message="messages.plugins.updateAvailable">
              <template #description>
                <div class="stack gap-1">
                  <span class="row gap-1 text-sm">
                    <span class="text-ink-dim">{{ messages.plugins.version }}</span>
                    <span class="font-mono">{{ plugin.update.version }}</span>
                  </span>

                  <template v-if="plugin.update.needsNewConsent">
                    <span class="text-sm">{{ messages.plugins.updateNeedsConsent }}</span>
                    <span class="row flex-wrap gap-1">
                      <ATag v-for="capability in plugin.update.addedCapabilities" :key="capability" class="font-mono">
                        {{ capability }}
                      </ATag>
                    </span>
                  </template>
                </div>
              </template>
            </AAlert>
          </div>
        </AList.Item>
      </template>
    </AList>
    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.plugins.publishers.title }}
      </h2>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.publishers.detail }}
      </p>

      <!--
        What this device knows about the keys that sign packages. `seen` means "a verified signature came from this key and
        nobody has decided yet"; the decision is the user's, and it is per key rather than per publisher name.
      -->
      <AEmpty v-if="publishers.length === 0" :description="messages.plugins.publishers.empty" />
      <AList v-else size="small">
        <AList.Item v-for="publisher in publishers" :key="publisher.publisherId">
          <div class="row-between gap-3">
            <span class="min-w-0 stack gap-0.5">
              <span class="truncate text-sm">{{ publisher.publisherId }}</span>
              <span class="truncate text-xs text-ink-dim font-mono">{{ publisher.fingerprint }}</span>
            </span>
            <span class="row gap-2">
              <ATag class="m-0">
                {{ publisher.trust }}
              </ATag>
              <AButton
                v-if="publisher.trust !== 'trusted'"
                size="small"
                :loading="deciding === publisher.publisherId"
                @click="decidePublisher(publisher.publisherId, 'trusted')"
              >
                {{ messages.plugins.publishers.trust }}
              </AButton>
              <AButton
                v-if="publisher.trust !== 'revoked'"
                size="small"
                :loading="deciding === publisher.publisherId"
                @click="decidePublisher(publisher.publisherId, 'revoked')"
              >
                {{ messages.plugins.publishers.revoke }}
              </AButton>
            </span>
          </div>
        </AList.Item>
      </AList>
    </section>
  </section>
</template>

<script setup lang="ts">
/**
 * What is installed, and what each one has been allowed to do.
 *
 * # Why granted and denied are shown rather than only granted
 *
 * A permissions screen that lists what a plugin *may* do tells the user nothing about what it *has tried* to do. The
 * design document asks for both, and it is the denied column that turns the screen from a settings page into the thing a
 * user actually reads before installing something.
 *
 * # Why the lifecycle state is a field and not a colour
 *
 * The second plugin in the fake snapshot is `enabled` **and** `faulted`. A screen that derived its badge from `enabled`
 * would show that plugin as working, which is the specific mistake `PluginSnapshot.state` exists to prevent.
 */
import type { CatalogItem, CatalogSort, InstallPreviewDto, InstallReport, PluginSnapshot, PublisherDto, SuggestedAction, UpdateInfo } from '@synra/protocol'
import type { AuditEntry, Failure } from './plugins/actions'
import { Alert as AAlert, Button as AButton, Checkbox as ACheckbox, CheckboxGroup as ACheckboxGroup, Empty as AEmpty, Input as AInput, List as AList, Modal as AModal, Segmented as ASegmented, Select as ASelect, Switch as ASwitch, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, onMounted, reactive, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { formatBytes } from '../format'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { usePluginsStore } from '../stores/plugins'
import { AUDIT_PAGE, clearAudit, decide, installFrom, installFromRegistry, planSync, previewInstall, audit as readAudit, runSync, searchCatalog, setEnabled, uninstall } from './plugins/actions'

const pluginsStore = usePluginsStore()
const { plugins } = storeToRefs(pluginsStore)

/** Which capabilities the user has ticked, per plugin. Seeded from `granted` when the cards arrive. */
const selection = reactive<Record<string, string[]>>({})

/**
 * The plugins as the list takes them.
 *
 * Copied because `AList`'s `dataSource` is typed as a mutable array and what the kernel answered is `readonly`. The
 * copy is made when the list changes rather than on every render.
 */
const installed = computed(() => [...plugins.value])

/** The filter the user typed, matched against id, title and description. */
const query = ref('')

/** What the list draws: the installed plugins the filter keeps. */
const visible = computed(() => {
  const needle = query.value.trim().toLowerCase()
  if (needle === '')
    return installed.value
  return installed.value.filter(plugin =>
    plugin.pluginId.toLowerCase().includes(needle)
    || plugin.title.toLowerCase().includes(needle)
    || (plugin.description ?? '').toLowerCase().includes(needle)
  )
})

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** Whether the list is being read, so the empty state's button can say that it is doing something. */
const loading = ref(false)

/**
 * The lifecycle state's tag colour.
 *
 * `enabled` is deliberately not consulted: a plugin that is enabled **and** faulted is exactly what
 * `PluginSnapshot.state` exists to report. The two states this milestone knows get a status colour and anything the
 * kernel invents later stays neutral, which is the honest default — the tag always carries the word (R23), so the
 * colour is redundancy rather than the state itself.
 */
function stateColour(state: string): 'success' | 'error' | 'default' {
  switch (state) {
    case 'active': return 'success'
    case 'faulted': return 'error'
    default: return 'default'
  }
}

/** The publishers whose signatures this device has seen. */
const publishers = ref<readonly PublisherDto[]>([])

/** The publisher whose decision is being written. */
const deciding = ref<string | undefined>(undefined)

/** The signing key of the package just installed, when it has not been decided yet. */
const pendingPublisher = ref<{ id: string, fingerprint: string } | undefined>(undefined)

/** Trusts the key the install just showed, and closes the prompt. */
async function trustPendingPublisher(): Promise<void> {
  const pending = pendingPublisher.value
  if (!pending)
    return
  await decidePublisher(pending.id, 'trusted')
  pendingPublisher.value = undefined
}

/** Revokes the key the install just showed — the opposite answer, from the same prompt. */
async function revokePendingPublisher(): Promise<void> {
  const pending = pendingPublisher.value
  if (!pending)
    return
  await decidePublisher(pending.id, 'revoked')
  pendingPublisher.value = undefined
}

onMounted(load)

/** Reads the installed plugins and the publishers this device has met. */
async function load(): Promise<void> {
  loading.value = true
  error.value = undefined

  const [answer, publisherAnswer] = await Promise.all([
    kernel.call<readonly PluginSnapshot[]>('plugins_list'),
    kernel.call<readonly PublisherDto[]>('plugins_publishers')
  ])
  loading.value = false

  if (answer.ok) {
    pluginsStore.applySnapshot(answer.value)
    // ★ The boxes start from what was already granted, so a second visit shows the decision rather than an empty form.
    for (const plugin of answer.value) {
      if (!(plugin.pluginId in selection))
        selection[plugin.pluginId] = [...plugin.granted]
    }
  }
  else {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
  }
  if (!publisherAnswer.ok) {
    if (answer.ok) {
      error.value = {
        code: publisherAnswer.error.code,
        message: publisherAnswer.error.message,
        actions: publisherAnswer.error.actions
      }
    }
  }
  else if (Array.isArray(publisherAnswer.value)) {
    publishers.value = publisherAnswer.value
  }
}

/** Trusts or revokes a publisher's key, then re-reads so the row shows the decision the kernel stored. */
async function decidePublisher(publisherId: string, trust: 'trusted' | 'revoked'): Promise<void> {
  deciding.value = publisherId
  error.value = undefined
  const answer = await kernel.call<null>('plugins_publisher_decide', { publisherId, trust })
  deciding.value = undefined
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  await load()
}

/** The plugin whose decision is being written, so its button says so and cannot be pressed twice. */
const consenting = ref<string | undefined>(undefined)

/**
 * Records the ticked capabilities and replaces the card with the answer's own.
 *
 * The answer **is** the card, so the list is replaced rather than re-read: one call, and what the user sees is exactly
 * what the kernel just recorded — a re-read would answer the same question a moment later and about every plugin.
 */
async function consent(plugin: PluginSnapshot, granted: readonly string[]): Promise<void> {
  consenting.value = plugin.pluginId
  error.value = undefined
  const outcome = await decide(kernel, plugin.pluginId, granted)
  consenting.value = undefined
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  pluginsStore.upsert(outcome.value)
  selection[outcome.value.pluginId] = [...outcome.value.granted]
}

/** The plugin whose switch is being written, so it shows a spinner and cannot be pressed twice. */
const toggling = ref<string | undefined>(undefined)

/**
 * Turns a plugin on or off.
 *
 * The list is **not** re-read here: the command publishes `PluginUpdated` and the page's own subscription reloads on
 * it. Replacing the card locally as well would be a second description of the same change, and the two would disagree
 * the first time the kernel answered something other than what the click assumed.
 */
async function toggle(plugin: PluginSnapshot, enabled: boolean): Promise<void> {
  toggling.value = plugin.pluginId
  error.value = undefined
  const outcome = await setEnabled(kernel, plugin.pluginId, enabled)
  toggling.value = undefined
  if (!outcome.ok)
    error.value = outcome.failure
}

/**
 * What to tell the user after an action that reported rather than changed a card: an install, a sync or a removal.
 *
 * One ref rather than two, because the page shows one note under the form: two would fight for the same line, and a
 * message that replaced another without being read is a message that was never shown.
 */
const note = ref<string | undefined>(undefined)

/**
 * The plugin whose removal is being decided, and whether its data goes with it.
 *
 * Two refs beside the list rather than one object, because the dialog opens on one plugin and the checkbox is answered
 * after it opens: a single object would have to be replaced on every tick, and `removing` is what the template tests.
 */
const removing = ref<PluginSnapshot | undefined>(undefined)

/** Whether the removal being decided also deletes the plugin's data. Reset every time the dialog opens. */
const removeData = ref(false)

/** Opens the removal dialog for one plugin, with the reversible answer preselected. */
function askRemove(plugin: PluginSnapshot): void {
  removeData.value = false
  removing.value = plugin
}

/**
 * The quota a health report carries, as the few numbers worth showing.
 *
 * `usage` is the quota as JSON so the DTO does not freeze a field list that the quota layer owns; this reads the four a
 * user can act on and shows nothing when the report has none — an empty "Limits" row would be a label with no fact.
 */
function healthLimits(health: PluginSnapshot['health']): string {
  if (health === undefined)
    return ''
  const usage = health.usage
  if (typeof usage !== 'object' || usage === null)
    return ''
  const number = (key: string): number | undefined => {
    const value = (usage as Record<string, unknown>)[key]
    return typeof value === 'number' ? value : undefined
  }
  const parts: string[] = []
  const memory = number('maxMemoryBytes')
  if (memory) {
    parts.push(`${formatBytes(memory)} ${messages.plugins.limitMemory}`)
  }
  const instances = number('maxInstances')
  if (instances) {
    parts.push(`${instances} ${messages.plugins.limitInstances}`)
  }
  const calls = number('hostCallsPerSec')
  if (calls) {
    parts.push(`${calls} ${messages.plugins.limitCalls}`)
  }
  const timeout = number('callTimeoutMs')
  if (timeout) {
    parts.push(`${timeout / 1000} ${messages.plugins.limitCallTime}`)
  }
  return parts.join(' · ')
}

/**
 * Removes the plugin, with or without its data.
 *
 * The decision is the dialog's, passed through rather than defaulted at the call: `false` here and `false` in the
 * command's own default would be two places that can disagree with the sentence beside the button.
 */
async function remove(): Promise<void> {
  const plugin = removing.value
  if (plugin === undefined)
    return
  const deleteData = removeData.value
  error.value = undefined
  const outcome = await uninstall(kernel, plugin.pluginId, deleteData)
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  // ★ No event for a removal — there is no card for a plugin that no longer exists — so the store is told directly, then
  // the list is read once. Forgetting here rather than only reloading means the card disappears even if the read fails.
  pluginsStore.forget(plugin.pluginId)
  removing.value = undefined
  const freed = formatBytes(outcome.value.bytesFreed)
  note.value = deleteData
    ? messages.plugins.removedDeleted(plugin.title, freed)
    : messages.plugins.removedKept(plugin.title, freed)
  await load()
}

/** The plugin whose page is being opened, so its button says so and cannot be pressed twice. */
const opening = ref<string | undefined>(undefined)

/**
 * Opens the plugin's page in its own window.
 *
 * The window is the shell's to build (`plugin_ui_open`), not this page's: the label, the origin and the token are decided
 * before the WebView exists, and a page that assembled them would be a second place those rules live. This is why the
 * call answers `null` and the failure is shown here — the button is the request, not the window.
 */
async function open(plugin: PluginSnapshot): Promise<void> {
  opening.value = plugin.pluginId
  error.value = undefined
  const answer = await kernel.call<null>('plugin_ui_open', { pluginId: plugin.pluginId })
  opening.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/** The plugin whose artifact is being sent, so its button says so and cannot be pressed twice. */
const syncing = ref<string | undefined>(undefined)

/**
 * Sends the artifact to the paired devices the plan says need it.
 *
 * ★ The plan is read before it is run, because it is the only place `online` is visible: a run over a plan whose targets
 * are all offline would report a failure per device for a network the user could have seen was down. When nothing needs
 * sending the button says so rather than showing a report of zeroes.
 */
async function sync(plugin: PluginSnapshot): Promise<void> {
  syncing.value = plugin.pluginId
  error.value = undefined
  note.value = undefined

  const planned = await planSync(kernel, plugin.pluginId)
  if (!planned.ok) {
    syncing.value = undefined
    error.value = planned.failure
    return
  }
  if (!planned.value.targets.some(target => target.needsArtifact)) {
    syncing.value = undefined
    note.value = messages.plugins.syncNothing
    return
  }

  const report = await runSync(kernel, planned.value)
  syncing.value = undefined
  if (!report.ok) {
    error.value = report.failure
    return
  }
  note.value = messages.plugins.syncDone(
    report.value.updated,
    report.value.unchanged,
    report.value.failures?.length ?? 0
  )
}

/** The directory the user typed. */
const installPath = ref('')

/** Where the install comes from: a directory this device has, or the registry it was configured with. */
const installMode = ref<'path' | 'registry'>('path')

/** The mode's labels, as the segmented control shows them. */
const installOptions = computed(() => [
  { label: messages.plugins.installMode.path, value: 'path' },
  { label: messages.plugins.installMode.registry, value: 'registry' }
])

/** Whether an install is in flight, so the button says so and cannot be pressed twice. */
const installing = ref(false)

/** The update the registry published, by plugin, for the cards that were asked about. */
const updates = ref<Record<string, UpdateInfo>>({})

/** What each plugin's audit ring said, once asked. `undefined` means "not asked yet". */
const audit = ref<Record<string, AuditEntry[] | undefined>>({})

/** Whether a plugin's history has been read to its end. Absent means "not known yet", which draws the button. */
const auditDone = ref<Record<string, boolean>>({})

/** The plugin whose next page is in flight. */
const loadingMoreAudit = ref<string | undefined>(undefined)

/**
 * Loads a plugin's recent calls whenever its section is opened.
 *
 * ★ The first page replaces whatever was there: reopening the section is the user asking "what has it been doing lately",
 * and appending to a list from before they closed it would mix two reads.
 */
async function openAudit(pluginId: string, event: Event): Promise<void> {
  if (!(event.target as HTMLDetailsElement).open)
    return
  const outcome = await readAudit(kernel, pluginId)
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  audit.value[pluginId] = [...outcome.value]
  // A short page is the only "no more" signal a limit-only query gives.
  auditDone.value[pluginId] = outcome.value.length < AUDIT_PAGE
}

/** Appends the next page of one plugin's history. */
async function loadMoreAudit(pluginId: string): Promise<void> {
  loadingMoreAudit.value = pluginId
  error.value = undefined
  const outcome = await readAudit(kernel, pluginId, AUDIT_PAGE, audit.value[pluginId]?.length ?? 0)
  loadingMoreAudit.value = undefined
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  // ★ Appended rather than replaced: the offset is "how many rows this card has already shown", and the page is a
  // continuation of the same order. A failed read leaves the rows that did arrive, because they are still true.
  audit.value[pluginId] = [...(audit.value[pluginId] ?? []), ...outcome.value]
  auditDone.value[pluginId] = outcome.value.length < AUDIT_PAGE
}

/** The plugin whose history is being cleared. */
const clearingAudit = ref<string | undefined>(undefined)

/** Forgets a plugin's history, and redraws the section as empty rather than re-reading it. */
async function clearTrail(pluginId: string): Promise<void> {
  clearingAudit.value = pluginId
  error.value = undefined
  const outcome = await clearAudit(kernel, pluginId)
  clearingAudit.value = undefined
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  // ★ Emptied here rather than re-read: the command just removed every record, so a read would ask the ring for a list
  // whose answer is already known. `[]` and not `undefined`, because "asked and empty" is the state that draws the
  // sentence; `undefined` would put the section back to "not asked yet".
  audit.value[pluginId] = []
}

/** The plugin whose update is being checked. */
const checking = ref<string | undefined>(undefined)

/** Asks the registry about one plugin, and shows or forgets the last answer. */
async function checkUpdate(plugin: PluginSnapshot): Promise<void> {
  checking.value = plugin.pluginId
  error.value = undefined
  const answer = await kernel.call<UpdateInfo | null>('plugins_check_update', {
    pluginId: plugin.pluginId
  })
  checking.value = undefined
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  if (answer.value) {
    updates.value = { ...updates.value, [plugin.pluginId]: answer.value }
    return
  }
  // No newer version: the previous answer, if any, is no longer true.
  const next = { ...updates.value }
  delete next[plugin.pluginId]
  updates.value = next
}

/** The plugin whose update is being installed. */
const updating = ref<string | undefined>(undefined)

/** Installs the newer version, then re-reads the list — the row the user sees comes from the store. */
async function applyUpdate(plugin: PluginSnapshot): Promise<void> {
  updating.value = plugin.pluginId
  error.value = undefined
  const answer = await kernel.call<null>('plugins_update', { pluginId: plugin.pluginId })
  updating.value = undefined
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  const next = { ...updates.value }
  delete next[plugin.pluginId]
  updates.value = next
  await load()
}

/** The package a registry install would fetch, shown before anything is downloaded. */
const preview = ref<InstallPreviewDto | undefined>(undefined)

/** What the registry's index returned for the last search, and whether one has run. */
const searchText = ref('')
const searchSort = ref<CatalogSort>('relevance')
const searched = ref(false)
const searching = ref(false)
const results = ref<CatalogItem[]>([])

/** How many results one page asks for; `hasMore` reads the last page's length against it. */
const SEARCH_PAGE = 20

/** The sorts the search endpoint can honour, named in the interface's language. */
const sortOptions = computed(() => [
  { label: messages.plugins.catalog.byRelevance, value: 'relevance' },
  { label: messages.plugins.catalog.byName, value: 'name' }
])

/** Whether the last page was full, which is the only signal a search without a total gives that more may exist. */
const hasMore = computed(() => results.value.length > 0 && results.value.length % SEARCH_PAGE === 0)

/** Searches the configured registry from the first page, and says so when nothing matched. */
async function search(): Promise<void> {
  const text = searchText.value.trim()
  if (text === '' || searching.value)
    return
  searching.value = true
  error.value = undefined
  const outcome = await searchCatalog(kernel, text, { offset: 0, sort: searchSort.value })
  searching.value = false
  searched.value = true
  if (!outcome.ok) {
    error.value = outcome.failure
    results.value = []
    return
  }
  results.value = [...outcome.value]
}

/** Asks for the next page and appends what was not already shown. */
async function loadMore(): Promise<void> {
  const text = searchText.value.trim()
  if (text === '' || searching.value)
    return
  searching.value = true
  error.value = undefined
  const outcome = await searchCatalog(kernel, text, {
    offset: results.value.length,
    sort: searchSort.value
  })
  searching.value = false
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  const known = new Set(results.value.map(item => item.pluginId))
  results.value = [...results.value, ...outcome.value.filter(item => !known.has(item.pluginId))]
}

/** Opens the permission dialog for a search hit — the same dialog an install by id uses. */
async function previewResult(item: CatalogItem): Promise<void> {
  error.value = undefined
  const outcome = await previewInstall(kernel, item.pluginId)
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  preview.value = outcome.value
}

/** Installs from the source the user named, then re-reads the list. */
async function install(): Promise<void> {
  const from = installPath.value.trim()
  if (from === '' || installing.value)
    return
  installing.value = true
  error.value = undefined

  // ★ A registry install asks first. The preview is the publisher's own claim about permissions, and a dialog that showed
  // them is the difference between "install this name" and "install this thing that wants these".
  if (installMode.value === 'registry') {
    const outcome = await previewInstall(kernel, from)
    installing.value = false
    if (!outcome.ok) {
      error.value = outcome.failure
      return
    }
    preview.value = outcome.value
    return
  }

  const outcome = await installFrom(kernel, from)
  installing.value = false
  await completeInstall(outcome.ok ? outcome.value : undefined, outcome.ok ? undefined : outcome.failure)
}

/** Installs what the preview described, after the user confirmed it. */
async function confirmPreview(): Promise<void> {
  const chosen = preview.value
  if (!chosen || installing.value)
    return
  preview.value = undefined
  installing.value = true
  error.value = undefined
  const outcome = await installFromRegistry(kernel, chosen.pluginId)
  installing.value = false
  await completeInstall(outcome.ok ? outcome.value : undefined, outcome.ok ? undefined : outcome.failure)
}

/** The steps both install sources share: the note, the list, and the publisher question. */
async function completeInstall(
  report: InstallReport | undefined,
  failure: Failure | undefined
): Promise<void> {
  if (!report) {
    if (failure)
      error.value = failure
    return
  }
  note.value = [messages.plugins.installedSwitchedOff, ...(report.warnings ?? [])].join(' ')
  installPath.value = ''
  await load()

  // ★ Asked after the list is re-read, so a key the user already trusted (or a package with no signature) does not
  // produce a second prompt. The fingerprint is the one the install verified, not one parsed here from the package.
  const known = report.publisherId
    ? publishers.value.find(publisher => publisher.publisherId === report.publisherId)
    : undefined
  if (report.publisherId && report.fingerprint && known?.trust !== 'trusted')
    pendingPublisher.value = { id: report.publisherId, fingerprint: report.fingerprint }
}
</script>
