import process from 'node:process'
import { checkCommandArguments } from './checks/arguments'
import { failures, notes } from './checks/common'
import { checkCommandNames, checkFakeIsQuarantined, checkInjection, checkSingleDeclarations, checkTypeOnlyImports } from './checks/contracts'
import { checkFacadeIsFresh } from './checks/facade'
import { checkControlNames, checkCornerLadder, checkDialogNames, checkFocusableNames, checkNativeButtons, checkPageButtons, checkTouchBreakpoints } from './checks/ui'

await checkInjection()
await checkSingleDeclarations()
await checkTypeOnlyImports()
const called = await checkCommandNames()
await checkCommandArguments(called)
await checkFakeIsQuarantined()
await checkFacadeIsFresh()
await checkPageButtons()
await checkDialogNames()
await checkControlNames()
await checkTouchBreakpoints()
await checkCornerLadder()
await checkNativeButtons()
await checkFocusableNames()
for (const note of notes) {
  console.log(`  · ${note}`)
}
if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`✗ ${failure.check}: ${failure.detail}`)
  }
  process.exit(1)
}
const total = notes.length + 1
console.log(`✓ shapes agree (${total.toString()} checks)`)
