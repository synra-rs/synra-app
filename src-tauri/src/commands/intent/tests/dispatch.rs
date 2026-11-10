use super::*;

#[test]
fn a_share_with_no_mime_is_sent_as_text() {
    // ★ The mismatch the module docs record. The interface's `Share` has no MIME and the kernel's item must, so the
    // shell picks one — and `text/plain` is the honest choice for "the user shared text". Sending a URI list because
    // the text happened to look like a URL is the guess this test exists to prevent.
    let state = crate::testing::state();
    let item = item_for(
        &state,
        &IntentSourceDto::Share {
            text: "https://example.com".to_owned(),
            title: None,
        },
    )
    .expect("text is routable");

    assert_eq!(item.mime, "text/plain");
    assert_eq!(item.text.as_deref(), Some("https://example.com"));
    assert!(item.transfers.is_empty());
}

#[test]
fn the_two_sources_this_build_cannot_route_are_refused() {
    // A file share needs a transfer the interface never starts, and a named plugin action is not the share entry
    // point. Both are refused rather than routed as something else: a `ShareItem` with no transfers would tell every
    // plugin the user shared nothing, which they would act on.
    let state = crate::testing::state();

    assert!(
        item_for(
            &state,
            &IntentSourceDto::ShareFiles {
                paths: vec!["/a".to_owned()]
            }
        )
        .is_err()
    );
    assert!(
        item_for(
            &state,
            &IntentSourceDto::PluginAction {
                plugin_id: synra_protocol::PluginId::parse("@acme/notes").expect("a plugin id"),
                action: "open".to_owned(),
                args: serde_json::json!({}),
            },
        )
        .is_err()
    );
}

#[test]
fn announcing_an_intent_the_board_cannot_answer_for_publishes_nothing() {
    // ★ The decision the helper documents: a board that lost the intent between the change and the announcement is a
    // race no caller can repair, and publishing an **error** is not what an event is for. A missed announcement costs
    // one stale list; a made-up one costs a wrong one.
    let state = crate::testing::state();
    let subscriber = state.events.subscribe();

    announce(&state, synra_protocol::ids::IntentId::new());

    assert!(
        state.events.take(subscriber).is_none(),
        "nothing was announced, because nothing was known"
    );
}

#[test]
fn a_share_is_dispatched_even_when_nothing_matches_it() {
    // ★ "This build cannot run plugins" and "no installed plugin wants that" are two facts a user acts on differently,
    // and only the first was tested before this.
    let state = engine_state();
    let subscriber = state.events.subscribe();

    // ★ The success path this file had none of: with an engine present a share is dispatched even when no plugin
    // matches — the intent exists and says so, which is a different answer from "this build cannot run plugins".
    let id = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect("an intent is raised even with nothing to match");

    assert!(
        intent_list(&state)
            .expect("the list")
            .iter()
            .any(|intent| intent.intent_id == id),
        "the board holds it"
    );
    let delivery = state
        .events
        .take(subscriber)
        .expect("and the window was told");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::IntentUpdated(intent)] => assert_eq!(intent.intent_id, id),
        other => panic!("expected the intent's card, got {other:?}"),
    }
}

#[test]
fn a_dispatch_records_its_outcome_and_duration() {
    // ★ The metric is written where the dispatch happened, with the daemon's own labels. Three claims: the outcome
    // counter fires, the no-handler counter fires when nothing offered, and the histogram received one observation —
    // a histogram with no observations is a shape rather than a measurement.
    use synra_observe::metrics::Value;

    let state = engine_state();
    let _ = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect("raised");

    let snapshot = state.metrics.snapshot();
    assert_eq!(
        snapshot.get("intents_total{outcome=ok}"),
        Some(&Value::Counter(1)),
        "routing succeeded, so the counter says so"
    );
    assert_eq!(
        snapshot.get("intent_no_handler_total"),
        Some(&Value::Counter(1))
    );
    match snapshot.get("intent_duration_ms") {
        Some(Value::Histogram(histogram)) => {
            assert_eq!(histogram.count, 1, "one dispatch, one observation")
        }
        other => panic!("expected a duration histogram, got {other:?}"),
    }
}

#[test]
fn a_command_without_an_engine_says_which_thing_is_missing() {
    // The shell is constructible without a plugin engine, and every intent command then refuses with the reason
    // rather than an empty answer. "No plugins are installed" and "this build cannot run them" are different facts
    // and a user acts on them differently.
    let state = crate::testing::state();
    let refused = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect_err("this fixture has no engine");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
}
