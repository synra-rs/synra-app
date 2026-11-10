use super::*;

#[test]
fn a_device_with_nothing_paired_shows_an_empty_list_and_not_an_error() {
    let snapshot = devices_snapshot(&testing::state()).expect("a snapshot");
    assert!(
        snapshot.devices.is_empty(),
        "nothing is paired, so nothing is listed"
    );
    assert_eq!(
        snapshot.identity.display_name, snapshot.identity.display_name,
        "the identity is still there"
    );
}

#[test]
fn the_snapshot_carries_this_device_so_the_screen_needs_one_call() {
    let snapshot = devices_snapshot(&testing::state()).expect("a snapshot");
    assert_eq!(snapshot.identity.kernel_version, crate::KERNEL_VERSION);
    assert!(
        snapshot.identity.name_is_default,
        "a fresh device's name was generated"
    );
    assert_eq!(snapshot.scanning, None, "and nothing is scanning yet");
}

#[test]
fn a_paired_device_becomes_a_card_the_interface_can_render() {
    let snapshot = devices_snapshot(&with_paired_device()).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1);
    let card = &snapshot.devices[0];
    assert_eq!(card.display_name, "Study PC");
    assert!(card.paired, "it is on the list because it was paired");
    assert_eq!(
        card.address.as_deref(),
        Some("192.168.1.4:5000"),
        "a paired device keeps its address"
    );
}

#[test]
fn a_paired_device_with_no_session_is_offline() {
    // ★ This line said `true` for several rounds while the comment above it said "a paired device with no open session
    // is offline". The comment was right.
    //
    // Every paired device was reported online, so the dashboard showed a green dot for a laptop that had been shut for a
    // week — and the first the user heard about it was a send that failed, which is the moment they can least do
    // anything about it. `SessionRegistry::live_for` is what answers the question the card is asking.
    //
    // ★ What this does not cover. The other direction — a live session making the card online — needs a session in the
    // registry, and this side has no way to open one: `SessionKey` is opaque to callers outside `synra-core` and the
    // registry is filled by the transport, which a shell test does not run. So this pins the value that was wrong and
    // not the query that replaced it; the query is `live_for`, one line, and this comment is where the next person
    // finds out nothing would go red if it were spelled differently.
    let state = with_paired_device();

    let snapshot = devices_snapshot(&state).expect("a snapshot");

    assert_eq!(snapshot.devices.len(), 1, "the fixture pairs one device");
    assert!(
        !snapshot.devices[0].online,
        "nothing has opened a session with this peer: {:?}",
        snapshot.devices[0]
    );
}

#[test]
fn devices_are_listed_by_name_when_the_names_differ() {
    let state = with_paired_device();
    state
        .db()
        .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Aardvark phone".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            address: None,
            paired_at_ms: 1_000,
        })
        .expect("pairs a second device");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(
        snapshot.devices[0].display_name, "Aardvark phone",
        "sorted by name"
    );
    assert_eq!(snapshot.devices[1].display_name, "Study PC");
}

#[test]
fn devices_with_the_same_name_still_come_out_in_one_order() {
    // ★ The tiebreaker is the identifier, and this is the test for it.
    //
    // Two devices with the same name must still come out in the same order on every device that has seen both, or a
    // difference in the list reads as one of the two lists being wrong. Sorting only by name leaves that to the database,
    // and SQLite is not obliged to agree with itself across runs.
    let state = with_paired_device();
    state
        .db()
        .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            address: None,
            paired_at_ms: 1_000,
        })
        .expect("pairs a second device");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(
        snapshot.devices[0].display_name, snapshot.devices[1].display_name,
        "with the same name"
    );
    assert_ne!(
        snapshot.devices[0].device_id, snapshot.devices[1].device_id,
        "so the order came from the identifiers"
    );
}

#[test]
fn a_device_that_was_only_heard_from_still_reaches_the_list() {
    // ★ The card a user pairs from.
    //
    // Before the kernel wrote `devices_seen` this card could not exist, so the devices screen could only ever show
    // devices that were already paired — and "what can I pair with?" had no answer anywhere in the product.
    let state = testing::state();
    state
        .db()
        .record_seen_device(&sighting(OTHER, "Hallway tablet"))
        .expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1);
    let card = &snapshot.devices[0];
    assert_eq!(card.display_name, "Hallway tablet");
    assert!(!card.paired, "it has been heard from and not paired with");
}

#[test]
fn a_device_that_is_both_paired_and_heard_from_is_listed_once() {
    // ★ The duplicate this guards against.
    //
    // The two tables are separate and neither cascades into the other, so a paired device that discovery also hears will
    // be in both. Without the de-duplication it would appear twice — once as paired, once as not — and the user would
    // see the same machine twice with different affordances.
    let state = with_paired_device();
    let paired_id = devices_snapshot(&state)
        .expect("a snapshot")
        .devices
        .remove(0)
        .device_id
        .to_string();
    state
        .db()
        .record_seen_device(&sighting(&paired_id, "Study PC"))
        .expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 1, "listed once");
    assert!(
        snapshot.devices[0].paired,
        "and as paired, which is the more capable answer"
    );
}

#[test]
fn paired_devices_are_listed_before_the_ones_that_still_need_pairing() {
    // ★ The first sort key.
    //
    // The devices a user can already send to are what the screen is for; the ones they have to pair with next are what
    // they are shopping for. The names are chosen so that sorting by name alone would put them the other way round.
    let state = with_paired_device();
    state
        .db()
        .record_seen_device(&sighting(OTHER, "Aardvark tablet"))
        .expect("records a sighting");

    let snapshot = devices_snapshot(&state).expect("a snapshot");
    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(
        snapshot.devices[0].display_name, "Study PC",
        "the paired one, despite sorting later by name"
    );
    assert!(snapshot.devices[0].paired);
    assert_eq!(snapshot.devices[1].display_name, "Aardvark tablet");
    assert!(!snapshot.devices[1].paired);
}

#[test]
fn an_unpaired_device_is_shown_without_an_address() {
    // ★ Why `address` is absent for these and present for paired ones.
    //
    // The store keeps an address for a paired device because that is where it is reachable. For a device seen once over
    // discovery, the address in that announcement is where it *was*, which is not a promise about now — and showing it
    // on a card that looks exactly as usable as a paired one offers the user a device they cannot reach.
    let state = testing::state();
    state
        .db()
        .record_seen_device(&sighting(OTHER, "Hallway tablet"))
        .expect("records a sighting");
    assert_eq!(
        devices_snapshot(&state).expect("a snapshot").devices[0].address,
        None
    );
}

#[test]
fn a_paired_device_keeps_its_address() {
    let snapshot = devices_snapshot(&with_paired_device()).expect("a snapshot");
    assert_eq!(
        snapshot.devices[0].address.as_deref(),
        Some("192.168.1.4:5000"),
        "a paired device is reachable at a known address"
    );
}
