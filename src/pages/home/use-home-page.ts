import type { IntentCandidate, IntentRetry, IntentSnapshot, ReceivedFile, SuggestedAction, TransferSnapshot } from '@synra/protocol'
import { storeToRefs } from 'pinia'
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { formatBytes } from '../../format'
import { kernel } from '../../kernel'
import { formatTime } from '../../locale-format'
import { messages } from '../../messages'
import { errorSentence } from '../../messages/errors'
import { useDevicesStore } from '../../stores/devices'
import { useIntentsStore } from '../../stores/intents'
import { useNoticesStore } from '../../stores/notices'
import { useSettingsStore } from '../../stores/settings'

export function useHomePage() {
  const router = useRouter()
  const devices = useDevicesStore()
  const settings = useSettingsStore()
  const askBeforeCancel = computed(() => settings.value('transfers.confirmCancel') !== false)
  const intentsStore = useIntentsStore()
  const { intents, transfers } = storeToRefs(intentsStore)
  const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)
  const received = ref<readonly ReceivedFile[]>([])
  async function refreshReceived(): Promise<void> {
    const answer = await kernel.call<readonly ReceivedFile[]>('transfer_received')
    if (answer.ok)
      received.value = answer.value
    else
      error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
  }
  const notices = useNoticesStore()
  watch(() => notices.latest, (latest) => {
    if (latest?.message === 'notice.received')
      void refreshReceived()
  })
  const cancelling = ref<string | undefined>(undefined)
  function percentOf(transfer: TransferSnapshot): number {
    if (transfer.totalBytes <= 0)
      return 0
    return Math.min(100, Math.round((transfer.receivedBytes / transfer.totalBytes) * 100))
  }
  function progressLabel(transfer: TransferSnapshot): string {
    return messages.home.inFlight.progress
      .replace('{done}', formatBytes(transfer.receivedBytes))
      .replace('{total}', formatBytes(transfer.totalBytes))
  }
  function directionOf(transfer: TransferSnapshot): string {
    return transfer.direction === 'receive' ? messages.home.inFlight.receiving : messages.home.inFlight.sending
  }
  function stateLabel(transfer: TransferSnapshot): string {
    return messages.home.inFlight.states[transfer.state as keyof typeof messages.home.inFlight.states] ?? transfer.state
  }
  async function cancelTransfer(transfer: TransferSnapshot): Promise<void> {
    cancelling.value = transfer.transferId
    error.value = undefined
    const answer = await kernel.call<null>('transfer_cancel', { transferId: transfer.transferId })
    cancelling.value = undefined
    if (!answer.ok)
      error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
  }
  const wire = computed(() => {
    const transfer = transfers.value[0]
    if (transfer)
      return { state: 'transferring' as const, device: devices.cards.find(card => card.deviceId === transfer.peerDevice) }
    const reachable = devices.cards.find(card => card.paired && card.online)
    if (reachable)
      return { state: 'connected' as const, device: reachable }
    const paired = devices.cards.find(card => card.paired)
    if (paired)
      return { state: 'offline' as const, device: paired }
    return { state: 'none' as const, device: undefined }
  })
  const state = computed(() => messages.home.wire.states[wire.value.state])
  const wirePeerName = computed(() => wire.value.state === 'transferring'
    ? wire.value.device?.displayName ?? transfers.value[0]?.peerName
    : wire.value.device?.displayName)
  const wireWidth = computed(() => {
    switch (wire.value.state) {
      case 'transferring': return 'w-1/2'
      case 'connected': return 'w-full'
      case 'offline': return 'w-full opacity-40'
      default: return 'w-1/12'
    }
  })
  const wireLive = computed(() => wire.value.state === 'connected' || wire.value.state === 'transferring')
  async function load(): Promise<void> {
    error.value = undefined
    const [intentAnswer, transferAnswer, receivedAnswer] = await Promise.all([
      kernel.call<readonly IntentSnapshot[]>('intent_list'),
      kernel.call<readonly TransferSnapshot[]>('transfer_list'),
      kernel.call<readonly ReceivedFile[]>('transfer_received')
    ])
    if (!intentAnswer.ok) {
      error.value = { code: intentAnswer.error.code, message: intentAnswer.error.message, actions: intentAnswer.error.actions }
      return
    }
    if (!transferAnswer.ok) {
      error.value = { code: transferAnswer.error.code, message: transferAnswer.error.message, actions: transferAnswer.error.actions }
      return
    }
    if (!receivedAnswer.ok) {
      error.value = { code: receivedAnswer.error.code, message: receivedAnswer.error.message, actions: receivedAnswer.error.actions }
      return
    }
    intentsStore.applySnapshot({ intents: intentAnswer.value, transfers: transferAnswer.value })
    received.value = receivedAnswer.value
  }
  const expanded = ref<string | undefined>(undefined)
  const copiedTrace = ref<string | undefined>(undefined)
  const selectedTrace = ref<string | undefined>(undefined)
  function toggleExpanded(intentId: string): void {
    expanded.value = expanded.value === intentId ? undefined : intentId
  }
  async function copyTrace(intentId: string, traceId: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(traceId)
    }
    catch {
      selectTrace(intentId)
      return
    }
    copiedTrace.value = traceId
    window.setTimeout(() => {
      if (copiedTrace.value === traceId)
        copiedTrace.value = undefined
    }, 2000)
  }
  function selectTrace(intentId: string): void {
    selectedTrace.value = intentId
    const node = document.querySelector(`[data-testid="trace-${CSS.escape(intentId)}"]`)
    if (!node)
      return
    const range = document.createRange()
    range.selectNodeContents(node)
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    window.setTimeout(() => {
      if (selectedTrace.value === intentId)
        selectedTrace.value = undefined
    }, 4000)
  }
  const deciding = ref<string | undefined>(undefined)
  async function decide(intent: IntentSnapshot, decision: Record<string, unknown>): Promise<void> {
    error.value = undefined
    deciding.value = intent.intentId
    const answer = await kernel.call<null>('intent_decide', {
      intentId: intent.intentId,
      decision
    })
    deciding.value = undefined
    if (!answer.ok) {
      error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
      return
    }
    await load()
  }
  async function accept(intent: IntentSnapshot, candidate: IntentCandidate): Promise<void> {
    await decide(intent, { decision: 'accept', deviceId: candidate.deviceId })
  }
  async function decline(intent: IntentSnapshot): Promise<void> {
    await decide(intent, { decision: 'decline', reason: null })
  }
  function canDecline(intent: IntentSnapshot): boolean {
    return !['succeeded', 'failed', 'noHandler', 'timedOut', 'refused', 'cancelled'].includes(intent.state)
  }
  function deviceName(deviceId: string): string {
    return devices.byId.get(deviceId)?.displayName ?? deviceId
  }
  function intentStateLabel(state: string): string {
    return (messages.home.received.states as Record<string, string | undefined>)[state] ?? state
  }
  function intentStateColor(state: string): string {
    if (state === 'succeeded')
      return 'success'
    if (state === 'failed' || state === 'timedOut' || state === 'refused')
      return 'error'
    if (state === 'noHandler')
      return 'warning'
    if (state === 'matching' || state === 'choosing' || state === 'confirming' || state === 'dispatching')
      return 'processing'
    return 'default'
  }
  function retryLabel(retry: IntentRetry): string {
    if (retry.gaveUp)
      return messages.home.received.noAnswer
    const template = retry.nextAtMs === undefined
      ? messages.home.received.retrying
      : messages.home.received.retryingAt
    return template
      .replace('{attempts}', String(retry.attempts))
      .replace('{max}', String(retry.maxAttempts))
      .replace('{time}', retry.nextAtMs === undefined ? '' : formatClock(retry.nextAtMs))
  }
  function formatClock(at: number): string {
    return formatTime(at, true)
  }
  function intentError(intent: IntentSnapshot): string {
    const error = intent.error
    if (!error)
      return ''
    return errorSentence(error.code)
  }
  onMounted(load)
  return { router, devices, askBeforeCancel, error, received, cancelling, percentOf, progressLabel, directionOf, stateLabel, cancelTransfer, wire, state, wirePeerName, wireWidth, wireLive, load, expanded, copiedTrace, selectedTrace, toggleExpanded, copyTrace, deciding, accept, decline, canDecline, deviceName, intentStateLabel, intentStateColor, retryLabel, formatClock, intentError, intents, transfers }
}
