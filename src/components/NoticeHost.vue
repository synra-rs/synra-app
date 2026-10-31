<template>
  <!--
    ★ The kernel's page-less notice, at the root beside the crash banner. A received file is news for whoever is looking at
    the screen, not for one page: the progress row disappears when the transfer finishes, and this is what replaces it.
  -->
  <AAlert
    v-if="notice"
    :type="alertType"
    show-icon
    closable
    :message="headline"
    :description="detail"
    class="mb-4 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8"
    @close="notices.dismiss()"
  >
    <template v-if="showAction" #action>
      <AButton size="small" @click="review">
        {{ messages.notices.review }}
      </AButton>
    </template>
  </AAlert>
</template>

<script setup lang="ts">
/**
 * Draws the latest `notice` event.
 *
 * # Why the message is a key and not a sentence
 *
 * The kernel sends `message` plus `params`, not English. The sentence is this side's, keyed by that word — the same rule the
 * error surface follows, and the reason a notice can be translated without touching the kernel.
 *
 * # Why an unknown key still shows something
 *
 * Because a kernel newer than the window may send a notice this build has never heard of. Printing the key would put an
 * engine word on screen (R34); showing nothing would make the event look lost. A generic sentence is the honest middle.
 */
import { Alert as AAlert, Button as AButton } from 'ant-design-vue'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import { messages } from '../messages'
import { useNoticesStore } from '../stores/notices'

const notices = useNoticesStore()
const router = useRouter()
const notice = computed(() => notices.latest)

/** A package notice has somewhere to go: the plugin that was installed or updated. */
const showAction = computed(() => {
  const message = notice.value?.message
  return message === 'notice.installed'
    || message === 'notice.updated'
    || message === 'notice.alreadyInstalled'
    || message === 'notice.newerInstalled'
    || message === 'notice.updates'
})

/** Opens the plugins page and clears the banner: the next step is on that page. */
function review(): void {
  const value = notice.value
  const pluginId = value?.params.pluginId
  notices.dismiss()
  void router.push(typeof pluginId === 'string'
    ? { path: '/plugins', query: { plugin: pluginId } }
    : '/plugins')
}

/** A link this build does not understand is a warning rather than a success. */
const alertType = computed(() => notice.value?.message === 'notice.unknownLink' ? 'warning' : 'success')

const headline = computed(() => {
  const value = notice.value
  if (!value)
    return ''
  if (value.message === 'notice.received')
    return messages.notices.received(String(value.params.name ?? ''))
  if (value.message === 'notice.installed')
    return messages.notices.installed(String(value.params.pluginId ?? ''), String(value.params.version ?? ''))
  if (value.message === 'notice.updated')
    return messages.notices.updated(String(value.params.pluginId ?? ''), String(value.params.version ?? ''))
  if (value.message === 'notice.alreadyInstalled')
    return messages.notices.alreadyInstalled(String(value.params.pluginId ?? ''), String(value.params.version ?? ''))
  if (value.message === 'notice.newerInstalled') {
    return messages.notices.newerInstalled(
      String(value.params.pluginId ?? ''),
      String(value.params.installed ?? ''),
      String(value.params.offered ?? '')
    )
  }
  if (value.message === 'notice.dropIgnored')
    return messages.notices.dropIgnored
  if (value.message === 'notice.updates')
    return messages.notices.updates(Number(value.params.count ?? 0))
  if (value.message === 'notice.unknownLink')
    return messages.notices.unknownLink(String(value.params.url ?? ''))
  return messages.notices.generic
})

const detail = computed(() => {
  const value = notice.value
  if (!value)
    return undefined
  if (value.message === 'notice.received') {
    const path = value.params.path
    return typeof path === 'string' ? messages.notices.savedTo(path) : undefined
  }
  // ★ The names, not only the count: "3 updates" makes the user open a page to find out which, and the pass already
  // carries the list. A comma-separated line is the smallest thing that answers the question the headline raises.
  if (value.message === 'notice.updates') {
    const plugins = value.params.plugins
    return Array.isArray(plugins) && plugins.length > 0 ? plugins.map(String).join(', ') : undefined
  }
  // ★ The install report's warnings are sentences from the verifier — "the package is not signed" is the one a user
  // most needs when a file was opened from the desktop. They are shown under the headline rather than dropped.
  if (value.message === 'notice.installed' || value.message === 'notice.updated') {
    const warnings = value.params.warnings
    return Array.isArray(warnings) && warnings.length > 0 ? warnings.map(String).join(' ') : undefined
  }
  return undefined
})
</script>
