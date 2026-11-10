use super::*;

#[test]
fn a_finished_scan_announces_the_cards_it_left_behind() {
    // ★ The publish the scan does after its network work, which no test could reach: extracted, it is called here on a
    // fixture with one paired device. The claim is the one that matters — a scan ends and the window is told what it
    // produced, one card per device, built by `devices_snapshot` rather than assembled separately.
    let state = with_paired_device();
    let subscriber = state.events.subscribe();

    super::super::announce_cards(&state);

    let delivery = state.events.take(subscriber).expect("a card was announced");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::DeviceCardUpdated(card)] => {
            assert_eq!(
                card.device_id.to_string(),
                state.identity.lock().device_id.to_string()
            );
        }
        other => panic!("expected one card, got {other:?}"),
    }
}

#[test]
fn a_scan_with_nothing_known_announces_nothing() {
    // ★ The other side of the same call: an empty record produces no events. A loop that published "the list is like this"
    // once would tell the window about nothing; this one is silent, which is what "nothing changed" should cost.
    let state = testing::state();
    let subscriber = state.events.subscribe();

    super::super::announce_cards(&state);

    assert!(
        state.events.take(subscriber).is_none(),
        "no devices, no announcements"
    );
}
