use super::*;

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
