use super::*;

#[test]
fn an_unknown_intent_is_refused_rather_than_answered_with_an_empty_one() {
    let state = crate::testing::state();
    let unknown = IntentId::new();

    assert!(
        intent_snapshot(&state, unknown).is_err(),
        "no such intent is not an empty snapshot"
    );
    assert!(intent_cancel(&state, unknown).is_err());
}

#[test]
fn an_empty_board_is_an_empty_list_rather_than_a_refusal() {
    // The panel this command exists for has nothing to show on a device that has never received anything, and
    // "nothing" is an answer. A refusal here would put an error on the home page of every fresh install.
    let state = crate::testing::state();

    assert!(
        intent_list(&state)
            .expect("an empty board is an empty list")
            .is_empty()
    );
}

#[test]
fn the_list_and_the_single_read_describe_an_intent_the_same_way() {
    // ★ Both commands build their answer through `snapshot_of`, and this is what says so: one mapping means a field
    // added to the DTO cannot appear in one answer and be missing from the other.
    let state = crate::testing::state();
    let (id, trace) = raise_an_intent(&state);

    let listed = intent_list(&state).expect("the board can be read");
    assert_eq!(
        listed.len(),
        1,
        "the one intent that was raised is the one that is listed"
    );
    // ★ `started_at_ms` is the moment the intent was **raised**. It used to be the moment the snapshot was read, so
    // two reads differed by however long the test took and this comparison had to normalise the field it was testing.
    let raised_at = state.board.lock().get(id).expect("raised").created_at_ms;
    let single = intent_snapshot(&state, id).expect("the same intent");
    assert_eq!(
        listed[0].started_at_ms, raised_at,
        "the raising time, not the read time"
    );
    assert_eq!(listed[0], single, "the two reads describe the same intent");
    // ★ The trace the board generated reaches the interface: it is the id a user pastes into `synra logs --trace`, and
    // a snapshot that dropped it left the filter with nothing anyone could know.
    assert_eq!(
        listed[0].trace_id, trace,
        "the board's trace is the snapshot's trace"
    );
}

#[test]
fn a_failed_intent_tells_the_page_why() {
    // ★ The field existed and was always `None`: the page's `intentError` line read a value no host ever filled. The
    // kernel's sentence travels as a detail; the code becomes the key the page translates.
    let state = crate::testing::state();
    let (id, _, created) = state.board.lock().raise(
        synra_core::intent::IntentSource::Direct {
            plugin_id: synra_protocol::PluginId::parse("@acme/opener").expect("a plugin id"),
            action: "open".to_owned(),
            params: serde_json::json!({}),
            target: synra_protocol::ids::DeviceId::new(),
        },
        synra_core::intent::Choice::Automatic,
        None,
        None,
        synra_store::sqlite::now_ms(),
    );
    assert!(created);
    state
        .board
        .lock()
        .conclude(
            id,
            Err(synra_protocol::SynraError::new(
                synra_protocol::ErrorCode::Internal,
                "the plugin trapped",
            )),
            3,
        )
        .expect("ended");

    let card = intent_snapshot(&state, id).expect("a card");
    let error = card.error.expect("the failure is carried");
    assert_eq!(error.code, synra_protocol::ErrorCode::Internal);
    assert_eq!(
        error.message.as_str(),
        "error.internal",
        "a key, not the kernel's sentence"
    );
    assert_eq!(
        error
            .details
            .as_ref()
            .and_then(|details| details.get("kernelMessage")),
        Some(&serde_json::json!("the plugin trapped")),
        "the sentence is kept for a bug report"
    );
}

#[test]
fn a_timed_out_intent_tells_the_page_how_long_it_waited() {
    // ★ The card's word is "Timed out"; the reason is the wait. Without this the user could not tell a matching
    // window that closed from a device that never answered — both said the same three syllables.
    let state = crate::testing::state();
    let (id, _) = raise_an_intent(&state);
    let _ = state.board.lock().time_out(id);

    let card = intent_snapshot(&state, id).expect("a card");
    assert_eq!(card.state, "timedOut");
    let error = card.error.expect("the wait is carried");
    assert_eq!(error.code, synra_protocol::ErrorCode::Timeout);
    assert_eq!(error.message.as_str(), "error.timeout");
    assert_eq!(
        error
            .details
            .as_ref()
            .and_then(|details| details.get("kernelMessage")),
        Some(&serde_json::json!("nothing answered within 400 ms"))
    );
}

#[test]
fn a_refused_intent_tells_the_page_which_check_refused_it() {
    // "Refused" without a reason is the same dead end as "Failed" without one: the refusal type documents its detail as
    // a sentence the interface can show, and the view was dropping both it and its code.
    let state = crate::testing::state();
    let (id, _) = raise_an_intent(&state);
    state.board.lock().refuse(
        id,
        synra_core::admission::AdmissionRefusal {
            step: synra_core::admission::AdmissionStep::PluginActive,
            detail: "the plugin is not active".to_owned(),
            code: synra_protocol::ErrorCode::PluginNotInstalled,
        },
    );

    let card = intent_snapshot(&state, id).expect("a card");
    assert_eq!(card.state, "refused");
    let error = card.error.expect("the refusal is carried");
    assert_eq!(error.code, synra_protocol::ErrorCode::PluginNotInstalled);
    assert_eq!(error.message.as_str(), "error.plugin_not_installed");
    assert_eq!(
        error
            .details
            .as_ref()
            .and_then(|details| details.get("kernelMessage")),
        Some(&serde_json::json!("the plugin is not active"))
    );
}

#[test]
fn a_successful_intent_has_no_error_to_show() {
    let state = crate::testing::state();
    let (id, _) = raise_an_intent(&state);
    state
        .board
        .lock()
        .conclude(id, Ok(serde_json::json!("opened")), 1)
        .expect("ended");

    assert!(intent_snapshot(&state, id).expect("a card").error.is_none());
}
