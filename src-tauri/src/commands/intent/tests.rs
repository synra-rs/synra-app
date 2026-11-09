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

/// A state with a real plugin engine that answers nothing.
///
/// Enough for `plugins(state)` to answer `Some`, which is what the success paths need: the commands above refuse
/// with `Unsupported` when there is no engine at all. No manifest and no plugin is installed — an empty set is a
/// *state*, not a stand-in.
fn engine_state() -> crate::AppState {
    engine_over(crate::testing::state())
}

/// The same engine over a state the caller built — a file-backed one, for a restart test.
fn engine_over(state: crate::AppState) -> crate::AppState {
    let backend = synra_plugin_host::Backend::new(std::sync::Arc::new(
        synra_plugin_host::engine::scripted::ScriptedBackend::new(),
    ));
    let set = synra_core::dispatch::PluginSet::new(
        backend,
        std::sync::Arc::new(synra_plugin_host::testing::RefusingHost),
    );
    state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(set)))
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

/// Puts one intent on the board directly.
///
/// `orchestrate::dispatch` is the share entry point and needs a plugin engine, which this fixture deliberately has none
/// of. `intent_list` reads the board, so the board is what the test writes — and `raise` is the kernel's own way to
/// write one, so what is listed is a real intent rather than a row shaped for the test.
fn raise_an_intent(state: &AppState) -> (IntentId, synra_protocol::ids::TraceId) {
    use synra_core::intent::{Choice, IntentSource};

    let item = ShareItem {
        from: state.identity.lock().device_id,
        mime: TEXT_MIME.to_owned(),
        text: Some("https://example.com".to_owned()),
        transfers: Vec::new(),
        context_id: None,
    };
    let (id, trace, created) = state.board.lock().raise(
        IntentSource::Share {
            item,
            source_app: None,
        },
        Choice::Automatic,
        None,
        None,
        synra_store::sqlite::now_ms(),
    );

    assert!(
        created,
        "a request that was never raised before raises a new intent"
    );
    (id, trace)
}

#[test]
fn an_intent_this_window_raised_is_still_there_after_a_restart() {
    // ★ The claim the shell could not make before: closing and reopening the window forgot every intent, so the home
    // page came back empty and `synra logs --trace` had nothing to find. A file-backed state and its reopen are the
    // only fixture that can see the difference between an in-memory board and a written one.
    let (root, state) = crate::testing::file_state();
    let state = engine_over(state);
    let id = intent_dispatch(
        &state,
        &IntentSourceDto::Share {
            text: "hi".to_owned(),
            title: None,
        },
    )
    .expect("an intent is raised");

    let reopened = crate::testing::reopen(&state);
    let listed = intent_list(&reopened).expect("the restored board answers");
    assert!(
        listed.iter().any(|intent| intent.intent_id == id),
        "the intent came back: {listed:?}"
    );
    drop(root);
}

#[test]
fn a_dispatch_restored_after_the_late_window_is_no_longer_running() {
    // ★ A restored `Dispatching` intent is work whose answer is still coming only for as long as the far side's late
    // queue keeps it. After that window, "running" is a sentence no fact can make true — and the row can never be
    // collected, because the retention sweep only takes finished intents.
    let (root, state) = crate::testing::file_state();
    let dispatched_at = crate::testing::FIXED_NOW_MS - synra_core::late_queue::MAX_AGE_MS - 1;
    let (id, _, created) = state.board.lock().raise(
        synra_core::intent::IntentSource::Direct {
            plugin_id: synra_protocol::PluginId::parse("@acme/opener").expect("a plugin id"),
            action: "open".to_owned(),
            params: serde_json::json!({"url": "https://example.com"}),
            target: synra_protocol::ids::DeviceId::new(),
        },
        synra_core::intent::Choice::Automatic,
        None,
        None,
        dispatched_at,
    );
    assert!(created, "a direct source is raised rather than matched");
    checkpoint(&state, id);

    let reopened = crate::testing::reopen(&state);
    let card = intent_list(&reopened)
        .expect("the restored board answers")
        .into_iter()
        .find(|intent| intent.intent_id == id)
        .expect("the intent came back");
    assert_eq!(
        card.state, "timedOut",
        "no answer can arrive any more: {card:?}"
    );
    drop(root);
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

#[test]
fn an_intent_a_peer_asked_this_window_to_run_is_written_down() {
    // ★ The remote direction, which is the one a restart hurts most: the peer is still waiting on the answer, and a
    // window that forgot the request would let the peer's retry run the action a second time. The listener calls
    // `announce` and `checkpoint` for exactly this path; this drives the same two functions over a real `accept_remote`.
    let (root, state) = crate::testing::file_state();
    let state = engine_over(state);
    let local = state.identity.lock().device_id;
    let peer = synra_protocol::ids::DeviceId::new();
    let request = synra_protocol::ids::RequestId::new();
    let mut envelope = synra_core::intent::remote::invoke(
        peer,
        local,
        &synra_core::intent::remote::Invoke {
            plugin_id: synra_protocol::PluginId::parse("@acme/opener").expect("a plugin id"),
            plugin_version: "1.0.0".to_owned(),
            action: "open".to_owned(),
            args: serde_json::json!({"url": "https://example.com"}),
        },
        synra_protocol::ids::TraceId::new(),
        1_000,
    )
    .expect("an invocation");
    envelope.request_id = request;

    let plugins = plugins(&state).expect("an engine");
    // An empty plugin set: the request is raised and then fails to execute, which is still an intent with a row —
    // and the lookup below is the one the listener uses.
    let _ = orchestrate::accept_remote(plugins, &state.board, local, &envelope, 1_000)
        .expect("the request is accepted");

    let id = {
        let board = state.board.lock();
        board.recall(peer, request).map(|intent| intent.id)
    }
    .expect("the request raised an intent");
    announce(&state, id);
    checkpoint(&state, id);

    let reopened = crate::testing::reopen(&state);
    let listed = intent_list(&reopened).expect("the restored board answers");
    assert!(
        listed.iter().any(|intent| intent.intent_id == id),
        "the peer's request came back: {listed:?}"
    );
    drop(root);
}
