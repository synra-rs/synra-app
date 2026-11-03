import type { PublisherDto } from '@synra/protocol'
import type { Failure } from './actions'
import { onScopeDispose, reactive, ref } from 'vue'
import { kernel } from '../../kernel'
import { failureOf } from './actions'

export function usePublishers() {
  const list = ref<readonly PublisherDto[]>([])
  const error = ref<Failure>()
  const errors = reactive<Record<string, Failure | undefined>>({})
  const deciding = ref('')
  let revision = 0
  onScopeDispose(() => revision++)
  async function read(): Promise<void> {
    const ticket = ++revision
    error.value = undefined
    const answer = await kernel.call<readonly PublisherDto[]>('plugins_publishers')
    if (ticket !== revision)
      return
    if (answer.ok)
      list.value = answer.value
    else
      error.value = failureOf(answer.error)
  }
  async function decide(publisherId: string, fingerprint: string, trust: 'trusted' | 'revoked'): Promise<boolean> {
    if (deciding.value)
      return false
    const ticket = revision
    deciding.value = publisherId
    delete errors[publisherId]
    const answer = await kernel.call<null>('plugins_publisher_decide', { publisherId, fingerprint, trust })
    deciding.value = ''
    if (ticket !== revision)
      return false
    if (!answer.ok) {
      errors[publisherId] = failureOf(answer.error)
      return false
    }
    await read()
    return true
  }
  return reactive({ list, error, errors, deciding, read, decide })
}
export type Publishers = ReturnType<typeof usePublishers>
