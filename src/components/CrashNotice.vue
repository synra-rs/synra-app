<template>
  <!--
    ★ The prompt 18 §7.3 asks for, at the root so it is visible on every page. It is a banner rather than a modal:
    the app is usable — safe mode has already decided not to load plugins — and a modal would make the user answer before
    they can read the plugins page that explains what happened.
  -->
  <AAlert
    v-if="notices.length > 0"
    type="warning"
    show-icon
    :message="messages.crash.title"
    class="mb-4"
  >
    <template #description>
      <div class="stack gap-2">
        <span>{{ messages.crash.detail(notices.length) }}</span>

        <ul v-if="details" class="m-0 stack gap-2 p-0">
          <li
            v-for="report in notices"
            :key="`${report.atMs}-${report.location ?? ''}`"
            class="stack gap-1 text-xs"
          >
            <span class="text-ink-dim font-mono">
              {{ formatDateTime(report.atMs) }} · {{ report.version }}
            </span>
            <span>{{ report.message ?? messages.crash.noMessage }}</span>
            <span v-if="report.location" class="text-ink-dim font-mono">{{ report.location }}</span>
          </li>
        </ul>

        <span v-if="bundlePath" class="text-xs text-ink-dim font-mono">{{ bundlePath }}</span>
        <span v-if="failure" class="text-error text-xs">{{ failure }}</span>

        <div class="row flex-wrap gap-2">
          <AButton size="small" @click="details = !details">
            {{ details ? messages.crash.hideDetails : messages.crash.showDetails }}
          </AButton>
          <AButton size="small" :loading="exporting" @click="exportBundle">
            {{ messages.crash.exportBundle }}
          </AButton>
          <AButton size="small" type="primary" :loading="dismissing" @click="dismiss">
            {{ messages.crash.ignore }}
          </AButton>
        </div>
      </div>
    </template>
  </AAlert>
</template>

<script setup lang="ts">
/**
 * "The last run ended unexpectedly", with the three actions 18 §7.3 names: look, export, ignore.
 *
 * # Why the banner filters by time
 *
 * Because `diag_crashes` returns every report on disk, and the prompt is about the run that just ended. A report from last
 * week is history; keeping it in the prompt would make the banner permanent, which is how a person learns to ignore it.
 * The five-minute window is the same one `StartupVerdict` uses — the two must agree about which crashes are "recent", or
 * the banner and safe mode would disagree about whether anything happened.
 *
 * # Why "ignore" is the only action that changes state
 *
 * The other two are reads. `diag_crash_dismiss` deletes the reports, and that is the only thing that makes the next start
 * skip the prompt and load plugins again; hiding the banner locally would bring it back next launch.
 */
import type { CrashReport, DiagExportResult } from '@synra/protocol'
import { Alert as AAlert, Button as AButton } from 'ant-design-vue'
import { onMounted, ref } from 'vue'
import { kernel } from '../kernel'

import { formatDateTime } from '../locale-format'
import { messages } from '../messages'

/** The same window `StartupVerdict` uses: a crash older than this is history, not the last run. */
const RECENT_MS = 5 * 60 * 1000

const notices = ref<readonly CrashReport[]>([])
const details = ref(false)
const dismissing = ref(false)
const exporting = ref(false)
const bundlePath = ref<string | undefined>(undefined)
const failure = ref<string | undefined>(undefined)

/** Keeps the reports that are about the run that just ended, newest first as the command returns them. */
function recentReports(reports: readonly CrashReport[]): readonly CrashReport[] {
  const cutoff = Date.now() - RECENT_MS
  // A report from the future means the clock moved backwards; treating it as recent is the safe direction (the same
  // choice `CrashReport::is_recent` makes).
  return reports.filter(report => report.atMs >= cutoff)
}

onMounted(async () => {
  const answer = await kernel.call<readonly CrashReport[]>('diag_crashes')
  // ★ A failed read is silent here. This is a notice, not a diagnostic command: an installation whose crash directory
  // cannot be read should still open, and turning the read's failure into an error banner would be a second prompt.
  if (answer.ok)
    notices.value = recentReports(answer.value)
})

async function dismiss(): Promise<void> {
  dismissing.value = true
  failure.value = undefined
  const answer = await kernel.call<null>('diag_crash_dismiss')
  dismissing.value = false
  if (!answer.ok) {
    failure.value = answer.error.message
    return
  }
  notices.value = []
  details.value = false
}

async function exportBundle(): Promise<void> {
  exporting.value = true
  failure.value = undefined
  const answer = await kernel.call<DiagExportResult>('diag_export', {
    options: { includeAudit: false, includeDebugLogs: false }
  })
  exporting.value = false
  if (!answer.ok) {
    failure.value = answer.error.message
    return
  }
  bundlePath.value = answer.value.path
}
</script>
