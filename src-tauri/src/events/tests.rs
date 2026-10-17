//! The bridge's tests: numbering, and what a `Behind` delivery still hands over.

use synra_core::bus::EventBus;

use super::*;

fn notice(message: &str) -> KernelEvent {
    KernelEvent::Notice { message: message.to_owned(), params: serde_json::json!({}) }
}

#[test]
fn an_envelope_carries_the_buses_number_for_each_event() {
    // ★ The number, not a counter of this module's own. The interface's tracker compares a sequence against the last it
    // applied, so a bridge that numbered from zero would make the first event of every run look like a repeat — and a
    // second publisher anywhere would make the numbers agree with nothing.
    let bus: EventBus<KernelEvent> = EventBus::new();
    let id = bus.subscribe();
    bus.emit(notice("first"));
    bus.emit(notice("second"));

    let envelopes = envelopes(&bus.take(id).expect("two events"), 7);

    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].seq, 1);
    assert_eq!(envelopes[1].seq, 2);
    assert_eq!(envelopes[0].at, 7, "the shell's clock, not the bus's");
}

#[test]
fn a_delivery_after_an_overflow_is_still_handed_over_with_its_numbers() {
    // The interface decides what to do about a gap — `must_refresh` is its own rule — so the bridge must not swallow
    // the survivors. What matters here is that their numbers are the numbers they were published with, not renumbered
    // from one.
    let bus: EventBus<KernelEvent> = EventBus::new();
    let id = bus.subscribe();
    for index in 1..=(synra_core::SUBSCRIBER_QUEUE_CAPACITY as u64 + 3) {
        bus.emit(notice(&format!("event {index}")));
    }

    let delivery = bus.take(id).expect("a delivery");
    let envelopes = envelopes(&delivery, 0);

    assert!(delivery.must_refresh(), "the queue overflowed");
    assert_eq!(envelopes.len(), synra_core::SUBSCRIBER_QUEUE_CAPACITY);
    assert_eq!(
        envelopes[0].seq,
        delivery.missed() + 1,
        "the first survivor keeps the number it was published with"
    );
}
