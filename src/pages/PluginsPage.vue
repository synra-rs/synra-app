<template>
  <section class="stack gap-6">
    <h1 class="text-xl font-semibold">
      {{ messages.plugins.title }}
    </h1>

    <!--
      The install form. A **path** rather than a package name: this shell has no registry client, and the command refuses a
      registry source by name instead of pretending to fetch one.
    -->
    <div class="row gap-2">
      <AInput v-model:value="installPath" :placeholder="messages.plugins.installPath" allow-clear @press-enter="install" />
      <AButton :loading="installing" :disabled="installPath.trim() === ''" @click="install">
        {{ installing ? messages.plugins.installing : messages.plugins.installAction }}
      </AButton>
    </div>

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
            </dl>

            <p v-if="plugin.description" class="m-0 text-sm text-ink-dim">
              {{ plugin.description }}
            </p>

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

              <APopconfirm
                :title="messages.plugins.removeTitle"
                :description="messages.plugins.removeKeepsData"
                :ok-text="messages.plugins.removeAction"
                @confirm="remove(plugin)"
              >
                <AButton size="small" danger>
                  {{ messages.plugins.removeAction }}
                </AButton>
              </APopconfirm>
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
import type { PluginSnapshot, SuggestedAction } from '@synra/protocol'
import { Alert as AAlert, Button as AButton, Checkbox as ACheckbox, CheckboxGroup as ACheckboxGroup, Empty as AEmpty, Input as AInput, List as AList, Popconfirm as APopconfirm, Switch as ASwitch, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, onMounted, reactive, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { usePluginsStore } from '../stores/plugins'
import { decide, installFrom, planSync, runSync, setEnabled, uninstall } from './plugins/actions'

/** A publisher row, as `plugins_publishers` answers it. */
interface PublisherRow {
  readonly publisherId: string
  readonly fingerprint: string
  readonly trust: string
  readonly firstSeenMs: number
  readonly decidedMs?: number
}

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
const publishers = ref<readonly PublisherRow[]>([])

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
    kernel.call<readonly PublisherRow[]>('plugins_publishers')
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
 * Removes the plugin, keeping its data.
 *
 * `remove_data` is `false` and not a choice here: the confirmation says the data stays, so asking for it to go would
 * contradict the sentence the user just agreed to. A checkbox for the other case belongs in a dialog that can show the
 * path, not in a one-line confirm.
 */
async function remove(plugin: PluginSnapshot): Promise<void> {
  error.value = undefined
  const outcome = await uninstall(kernel, plugin.pluginId, false)
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  // ★ No event for a removal — there is no card for a plugin that no longer exists — so the store is told directly, then
  // the list is read once. Forgetting here rather than only reloading means the card disappears even if the read fails.
  pluginsStore.forget(plugin.pluginId)
  await load()
}

/** The plugin whose page is being opened, so its button says so and cannot be pressed twice. */
const opening = ref<string | undefined>(undefined)

/**
 * What to tell the user after an action that reported rather than changed a card: an install, or a sync.
 *
 * One ref rather than two, because the page shows one note under the form: two would fight for the same line, and a
 * message that replaced another without being read is a message that was never shown.
 */
const note = ref<string | undefined>(undefined)

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

/** Whether an install is in flight, so the button says so and cannot be pressed twice. */
const installing = ref(false)

/** Installs the package in the directory the user named, then re-reads the list. */
async function install(): Promise<void> {
  const from = installPath.value.trim()
  if (from === '' || installing.value)
    return
  installing.value = true
  error.value = undefined
  const outcome = await installFrom(kernel, from)
  installing.value = false
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  note.value = messages.plugins.installedSwitchedOff
  installPath.value = ''
  await load()

  // ★ Asked after the list is re-read, so a key the user already trusted (or a package with no signature) does not
  // produce a second prompt. The fingerprint is the one the install verified, not one parsed here from the package.
  const signed = outcome.value
  const known = signed.publisherId
    ? publishers.value.find(publisher => publisher.publisherId === signed.publisherId)
    : undefined
  if (signed.publisherId && signed.fingerprint && known?.trust !== 'trusted')
    pendingPublisher.value = { id: signed.publisherId, fingerprint: signed.fingerprint }
}
</script>
