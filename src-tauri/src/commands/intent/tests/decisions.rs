use super::*;

#[test]
fn a_declined_intent_is_announced_in_its_new_state() {
    // The other half of the announce rule: a decision that **landed** is announced, with the state the board now
    // holds rather than the one it had before the decision.
    let state = engine_state();
    let id = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect("raised");
    let subscriber = state.events.subscribe();

    intent_decide(
        &state,
        id,
        &IntentDecisionDto::Decline {
            reason: Some("not now".to_owned()),
        },
    )
    .expect("declined");

    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::IntentUpdated(intent)] => {
            assert_eq!(intent.intent_id, id);
            assert_ne!(
                intent.state, "awaiting_choice",
                "the announcement carries the decision"
            );
        }
        other => panic!("expected the intent's card, got {other:?}"),
    }
}

#[test]
fn a_cancelled_intent_keeps_its_card_and_says_it_was_cancelled() {
    // ★ Cancellation is the one decision that does **not** remove the intent: `intent_list` is `all`, not the live
    // ones, so a panel that dropped it would lose the record of what the user stopped — the thing they are most likely
    // to look for. The announcement is therefore the same card with a different state.
    let state = engine_state();
    let id = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect("raised");
    let subscriber = state.events.subscribe();

    intent_cancel(&state, id).expect("cancelled");

    assert!(
        intent_list(&state)
            .expect("the list")
            .iter()
            .any(|intent| intent.intent_id == id),
        "it is still listed after being cancelled"
    );
    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::IntentUpdated(intent)] => assert_eq!(intent.intent_id, id),
        other => panic!("expected the intent's card, got {other:?}"),
    }
}
