/**
 * What the host advertises, checked against what it actually does.
 *
 * # The invariant this file is built around
 *
 * A host that reports `hostSupports('event.subscribe') === true` and then rejects the call is worse than one that never
 * claimed it: the author writes the branch, it passes review, and the failure lands on a user instead of in a review.
 * The other direction matters just as much — a call that quietly works while the table says it does not leaves an
 * author guarding against something that never fails, and refusing the API that would have worked.
 *
 * So it is tested as the biconditional it is: advertised means it works, and not-advertised means it refuses — both
 * halves, for every feature the interface declares, so a feature added to the library without a decision in the host
 * shows up as a missing entry.
 *
 * The surface itself is asserted in `injection-surface.test.ts` and the plain refusals in
 * `injection-refusals.test.ts`; the factories all three share are in `injection-test-helpers.ts`.
 */

import type { ErrorCode } from '@synra/protocol'
import { describe, expect, it } from 'vitest'
import { createInjection } from './injection'
import { CALLS, context, FEATURES } from './injection-test-helpers'

describe('what the host advertises and what it does', () => {
  for (const feature of FEATURES) {
    it(`\`${feature}\` is advertised exactly when the call works`, async () => {
      // The grants that remove "denied" from the picture, so what is left is "the host cannot do this yet" and not
      // "this plugin was not allowed".
      const synra = createInjection(context({ granted: ['device.pick', 'clipboard_read'] }))
      const advertised = synra.env.hostSupports(feature)

      const failure = await CALLS[feature](synra).then(
        () => undefined,
        (reason: unknown) => reason as { code?: ErrorCode }
      )

      if (advertised)
        expect(failure, `${feature} is advertised but the call rejected`).toBeUndefined()
      else
        expect(failure, `${feature} is not advertised but the call worked`).toBeDefined()
    })
  }

  it('keeps the device list and the supported list in step', () => {
    const synra = createInjection(context())
    expect([...synra.device.supported].sort()).toEqual(
      FEATURES.filter(feature => synra.env.hostSupports(feature)).sort()
    )
  })
})

describe('capabilities the plugin was refused', () => {
  // ★ The refusal names the capability, and says "denied" rather than "unsupported".
  //
  // Those are different problems with different fixes: a denied capability is a question for the user, an unsupported
  // one is a question for the host. Merging them means the author cannot tell whether asking again would help.
  it('clipboard.read is refused as denied without the grant', async () => {
    const synra = createInjection(context({ granted: [] }))
    await expect(synra.clipboard.read()).rejects.toMatchObject({ code: 'capability_denied' })
  })

  it('clipboard.read is refused as unsupported with the grant, because no command exists', async () => {
    const synra = createInjection(context({ granted: ['clipboard_read'] }))
    await expect(synra.clipboard.read()).rejects.toMatchObject({ code: 'unsupported' })
  })

  it('device.pick is refused as denied without the grant', async () => {
    const synra = createInjection(context({ granted: [] }))
    await expect(synra.device.pick()).rejects.toMatchObject({ code: 'capability_denied' })
  })
})
