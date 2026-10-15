/**
 * The channel kernel events arrive on: the contract between the shell and this interface.
 *
 * # Why this is its own file
 *
 * ★ It was `export const` in [`fake-kernel.ts`](./fake-kernel.ts), and **production code imported it from there**:
 * `DevicesPage.vue` and the injected plugin facade both did. That is the one thing `kernel.ts`'s own comment warns
 * against — "a component that reached for the fake directly would keep working after the real bridge exists, and would
 * keep working in the one place it must not". Reaching for a fake's *constant* is the smallest version of that mistake
 * and has the same consequence: the name of the channel is a fact about the shell, and a page that reads it from a test
 * double is a page whose subscription is coupled to a fixture that will be deleted or rewritten.
 *
 * Nothing about the name is fake. Both bridges deliver on it — `FakeKernel` because it is a stand-in for a shell, and
 * [`TauriKernel`](./tauri.ts) because it is the shell's own side. So it belongs where both can read it without either
 * owning it.
 *
 * # Why one channel rather than one per event kind
 *
 * The sequence number is a property of the **stream**, not of a kind. Per-kind channels would give each kind its own
 * sequence, so a gap in one would look like a fresh start in the next, and the interface's whole reason for tracking a
 * sequence — noticing that it missed something — would stop working for every kind but one.
 */

/** The one channel every kernel event arrives on. */
export const KERNEL_EVENT_CHANNEL = 'kernel-event'
