use super::*;

#[test]
fn forgetting_a_device_tells_the_interface() {
    // ★ The event side of forgetting. Without it the card stays on the list until the page is reopened — the same silent
    // staleness the plugin commands were fixed for.
    let state = testing::state();
    state
        .db()
        .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Phone".to_owned(),
            platform: "android".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            address: None,
        })
        .expect("pairs a device");
    let subscriber = state.events.subscribe();

    super::super::devices_forget(&state, OTHER).expect("forgets it");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::DeviceRemoved { device_id }) => {
            assert_eq!(
                device_id.to_string(),
                OTHER,
                "and names the device that went"
            );
        }
        other => panic!("expected a removal, got {other:?}"),
    }
}

#[test]
fn a_forgotten_device_is_gone_from_the_next_snapshot() {
    let state = with_paired_device();
    let card = devices_snapshot(&state)
        .expect("a snapshot")
        .devices
        .remove(0);
    let id = card.device_id.to_string();

    devices_forget(&state, &id).expect("forgets");
    assert!(
        devices_snapshot(&state)
            .expect("a snapshot")
            .devices
            .is_empty(),
        "the pairing is gone"
    );
}

#[test]
fn forgetting_survives_a_restart() {
    let (root, state) = testing::file_state();
    state
        .db()
        .pair_device(&Paired {
            device_id: OTHER.to_owned(),
            display_name: "Study PC".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            address: None,
        })
        .expect("pairs");

    devices_forget(&state, OTHER).expect("forgets");
    let reopened = testing::reopen(&state);
    assert!(
        devices_snapshot(&reopened)
            .expect("a snapshot")
            .devices
            .is_empty(),
        "and it did not come back when the shell started again"
    );
    drop(root);
}

#[test]
fn forgetting_a_device_that_does_not_parse_is_the_callers_fault() {
    let error = devices_forget(&testing::state(), "not-a-device").expect_err("a bad identifier");
    assert_eq!(
        error.code,
        ErrorCode::InvalidArgument,
        "the caller sent it and the caller can fix it — unlike a corrupt row, which is nobody's"
    );
}

#[test]
fn forgetting_an_unknown_device_succeeds_and_removes_nothing() {
    // Not an error: the user's intent — "this device is not here" — is satisfied. Refusing would make a retry loop
    // possible on a state that is already the one the user asked for.
    devices_forget(&testing::state(), OTHER).expect("forgetting nothing is still forgetting");
}
