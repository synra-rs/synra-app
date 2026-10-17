//! What the kernel tells the window about, as it happens.
//!
//! # Why this module exists at all
//!
//! The frontend has had a `SequenceTracker` and an event dispatcher for two rounds and **no producer**: every page was a
//! snapshot taken when it was opened, so a plugin enabled in another window or a scan that finished while a dialog was
//! open stayed invisible until the user navigated away and back.
//!
//! # Why the wire events and not the kernel's own
//!
//! Because the interface is this stream's only consumer. `synra-ipc`'s `KernelEvent` is shaped for the screen — a
//! device card, a plugin card, an intent snapshot — while `synra_core::KernelEvent` is shaped for the headless kernel,
//! which has no cards. Publishing the wire shape means there is no mapping layer to drift; publishing the kernel shape
//! would mean writing one and maintaining it twice.
//!
//! # Why a thread rather than an async task
//!
//! Because the bus is a **poll** (`take` answers `None` when nothing is waiting), and a window has one of these for its
//! whole life. A thread that sleeps 20 ms between empty reads costs nothing a task would not, and it cannot be
//! starved by a plugin call holding the runtime's worker.

#[cfg(feature = "tauri-shell")]
use std::sync::Arc;
#[cfg(feature = "tauri-shell")]
use std::time::Duration;

use synra_core::bus::Delivery;
#[cfg(feature = "tauri-shell")]
use synra_core::bus::EventBus;
use synra_ipc::event::{KernelEvent, KernelEventEnvelope};

/// The one channel every kernel event arrives on.
///
/// The same string the interface listens on (`src/ipc/channel.ts`, `KERNEL_EVENT_CHANNEL`). It is written here as well
/// because the two sides are compiled separately; a mismatch is a window that receives nothing, which is exactly the
/// state this module was added to end.
pub const CHANNEL: &str = "kernel-event";

/// How long the bridge waits before asking an empty queue again.
///
/// Twenty milliseconds is under a frame at 60 Hz and well under the point a person notices a list change. A longer wait
/// would be cheaper and visible; a busy loop would be neither.
#[cfg(feature = "tauri-shell")]
const POLL: Duration = Duration::from_millis(20);

/// The envelopes for one delivery, numbered from the bus's own count.
///
/// # Why `first_seq + index`
///
/// Because a subscriber's queue is contiguous — the events were emitted one after another — so the batch's first number
/// and its position are enough. A delivery that follows an overflow says so with [`Delivery::missed`], and the bridge
/// hands the events over regardless: the interface's tracker is what decides to re-read a snapshot.
#[must_use]
pub fn envelopes(delivery: &Delivery<KernelEvent>, at: i64) -> Vec<KernelEventEnvelope> {
    let first = delivery.first_seq();
    delivery
        .events()
        .iter()
        .enumerate()
        .map(|(index, event)| KernelEventEnvelope {
            seq: first.saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
            at,
            event: event.clone(),
        })
        .collect()
}

/// Starts the bridge: every event the kernel publishes reaches the window from now on.
///
/// The subscriber is never removed, because the thread lives as long as the process. A window that closed while the
/// shell kept running would still be delivering to an `AppHandle` that can no longer emit — which is why the loop ends
/// on the first failed emit rather than spinning.
#[cfg(feature = "tauri-shell")]
pub fn spawn(app: tauri::AppHandle, events: Arc<EventBus<KernelEvent>>) {
    use tauri::Emitter as _;

    let subscriber = events.subscribe();
    std::thread::spawn(move || loop {
        match events.take(subscriber) {
            Some(delivery) => {
                for envelope in envelopes(&delivery, synra_store::sqlite::now_ms()) {
                    if app.emit(CHANNEL, &envelope).is_err() {
                        // The window is gone. Nothing else can be delivered and the bus would only queue for a
                        // subscriber nobody reads.
                        return;
                    }
                }
            }
            None => std::thread::sleep(POLL),
        }
    });
}

#[cfg(test)]
mod tests;
