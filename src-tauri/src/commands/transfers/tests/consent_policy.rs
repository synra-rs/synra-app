use super::*;

#[test]
fn a_device_the_user_accepts_forever_is_not_asked_again() {
    // ★ The preference is per device and persisted under one configuration key: a fresh install asks, an accepted device
    // does not, and a different device still does.
    let state = crate::testing::state();
    let remembered = "11111111-1111-4111-8111-111111111111";
    let other = "22222222-2222-4222-8222-222222222222";
    consent::pair(&state, remembered);

    assert!(
        !super::super::auto_accept_peer(&state, remembered),
        "a fresh install asks"
    );
    super::super::remember_peer(&state.db(), remembered).expect("remembered");
    assert!(
        super::super::auto_accept_peer(&state, remembered),
        "the answer was remembered"
    );
    assert!(
        !super::super::auto_accept_peer(&state, other),
        "another device still asks"
    );

    // Read back through the settings snapshot, which is what a screen would draw.
    let config = crate::commands::config::config_get(&state).expect("the configuration reads");
    let accepted = config
        .values
        .get("transfers.acceptFrom")
        .and_then(|value| value.as_array())
        .expect("the preference is stored");
    assert_eq!(accepted, &vec![serde_json::json!(remembered)]);
}

#[test]
fn forgetting_a_device_forgets_that_it_was_trusted() {
    // ★ The preference is about a device; once the device is forgotten, the preference is about nothing. It is also
    // removed rather than left as an empty array, so a settings screen has nothing to interpret.
    let state = crate::testing::state();
    let peer = "11111111-1111-4111-8111-111111111111";
    consent::pair(&state, peer);

    super::super::remember_peer(&state.db(), peer).expect("remembered");
    assert!(super::super::auto_accept_peer(&state, peer));
    super::super::forget_sender(&state.db(), peer).expect("forgotten");

    assert!(
        !super::super::auto_accept_peer(&state, peer),
        "the device asks again"
    );
    let config = crate::commands::config::config_get(&state).expect("the configuration reads");
    assert!(
        config.values.get("transfers.acceptFrom").is_none(),
        "the empty preference was removed rather than stored"
    );
}

#[test]
fn answering_a_prompt_that_is_not_waiting_is_refused() {
    // ★ The prompt's id is one a panel was told about. Answering twice, or answering after the window closed, is an
    // answer to a question that no longer exists — and a kernel that said "done" would leave the panel believing it
    // settled something a peer is still waiting on.
    let state = crate::testing::state();
    let refused =
        super::super::transfer_respond(&state, "00000000-0000-4000-8000-000000000003", true, None)
            .expect_err("nothing is waiting");
    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);

    let invalid =
        super::super::transfer_respond(&state, "not-an-id", true, None).expect_err("it does not parse");
    assert_eq!(invalid.code, synra_protocol::ErrorCode::InvalidArgument);
}
