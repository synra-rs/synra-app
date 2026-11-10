use super::*;

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

mod decisions;
mod dispatch;
mod persistence;
mod snapshots;
