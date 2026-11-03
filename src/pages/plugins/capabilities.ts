import { messages } from '../../messages'

/** Unknown capabilities keep the full identifier; no scope is inferred from its spelling. */
export function capabilityPurpose(capability: string): string {
  const known = messages.plugins.workflow.capabilities
  const key = Object.keys(known).find(key => capability === key || capability.startsWith(`${key}:`)) as keyof typeof known | undefined
  return key ? known[key] : messages.plugins.workflow.permissionUnknown
}
