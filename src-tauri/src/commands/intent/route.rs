use super::{
    AppState, CandidateDto, IntentId, IntentSourceDto, IpcError, RouteError, ShareItem, TEXT_MIME,
    announce, checkpoint, orchestrate,
};

pub(super) fn as_ipc(error: &RouteError) -> IpcError {
    IpcError::unknown(error.code).with_params(serde_json::json!({ "reason": error.message }))
}

/// Reads a plugin set, or refuses the way every plugin command does.
pub(super) fn plugins(
    state: &AppState,
) -> Result<&std::sync::Arc<parking_lot::Mutex<synra_core::dispatch::PluginSet>>, IpcError> {
    state.plugin_set()
}

/// The share item a source describes.
///
/// # Why two of the four sources are refused
///
/// Because the kernel has nothing to route them to, and saying so is better than routing something else:
///
/// * `ShareFiles` needs transfers, and the interface has no path that uploads a file first. A `ShareItem` with an empty
///   transfer list would tell every plugin "the user shared nothing", which is a lie they would act on.
/// * `PluginAction` names a plugin and an action directly, so there is nothing to match — the board's own `raise` skips
///   matching for a `Direct` source. That path exists in the kernel and this command does not go through it, because
///   `orchestrate::dispatch` is the *share* entry point; invoking a named action is a different call than routing one.
pub(super) fn item_for(state: &AppState, source: &IntentSourceDto) -> Result<ShareItem, IpcError> {
    let from = state.identity.lock().device_id;
    let refused = |what: &str| {
        IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({ "reason": what }))
    };
    match source {
        IntentSourceDto::Share { text, title: _ } => Ok(ShareItem {
            from,
            // ★ See the module docs: the interface's `Share` has no MIME, so this is what the shell sends. A launcher
            // plugin that cares whether its input is a link should say so by its own declaration rather than by the
            // shell guessing from the text, which is how a string that merely looks like a URL gets opened.
            mime: TEXT_MIME.to_owned(),
            text: Some(text.clone()),
            transfers: Vec::new(),
            context_id: None,
        }),
        IntentSourceDto::Clipboard => {
            // The clipboard's content is read here rather than by a plugin, because a plugin that could read the
            // clipboard on its own would be able to read it whenever it liked. The user asked for this share, so the
            // shell reads it once, for this item.
            use synra_platform::Platform as _;
            // Made here rather than held on `AppState`: `CommandPlatform` is a desktop enum and a runner with no state,
            // and the only thing on it that accumulates is the notification rate, which reading the clipboard does not
            // touch. A field on the state would be one more thing for every command to thread.
            let text = synra_platform::CommandPlatform::default()
                .read_clipboard()
                .map_err(|error| {
                    IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
                        .with_params(serde_json::json!({ "reason": error.to_string() }))
                })?;
            // ★ `None` is an empty clipboard, which is not a share with no content.
            //
            // A `ShareItem` whose text is `None` tells every plugin the clipboard held nothing *of a kind it can use*,
            // which is a different fact from "the clipboard is empty" and one a plugin would act on differently. The
            // user asked to share what they had copied; there was nothing, and that is what they are told.
            let Some(text) = text else {
                return Err(refused("the clipboard is empty"));
            };
            Ok(ShareItem {
                from,
                mime: TEXT_MIME.to_owned(),
                text: Some(text),
                transfers: Vec::new(),
                context_id: None,
            })
        }
        IntentSourceDto::ShareFiles { .. } => Err(refused(
            "sharing files needs a transfer first, and no interface starts one yet",
        )),
        IntentSourceDto::PluginAction { .. } => Err(refused(
            "invoking a named plugin action is not the share entry point; nothing routes it yet",
        )),
        // ★ `IntentSourceDto` is `#[non_exhaustive]`. A source this build does not know is refused rather than
        // routed as a share: guessing what the user meant is how the wrong thing gets opened.
        _ => Err(refused("this build does not know that kind of share")),
    }
}

/// One candidate, as the interface shows it.
///
/// ★ **A candidate that named no device is reported as running here**, which is the kernel's own meaning: with no
/// suggestion the default is this machine, and the "where?" question exists precisely because that default may not be
/// what the user wants. The interface's `IntentCandidate` requires a device, so there is no way to say "no opinion" here
/// — and reporting a device the plugin did not name is a smaller distortion than reporting none at all, which the shape
/// does not permit.
pub(super) fn candidate_card(
    state: &AppState,
    candidate: &orchestrate::CandidateView,
) -> CandidateDto {
    let local = state.identity.lock().device_id;
    let device_id = candidate.suggested_target.unwrap_or(local);
    CandidateDto {
        plugin_id: candidate.plugin_id.clone(),
        title: candidate.title.clone(),
        device_id,
        // This device is reachable by definition; a peer is looked up. Answered rather than assumed, because a chooser
        // that offers an offline device is a chooser whose first choice fails.
        online: device_id == local
            || state
                .db()
                .paired_devices()
                .unwrap_or_default()
                .iter()
                .any(|row| row.device_id == device_id.to_string() && row.is_usable()),
    }
}

/// Hands the kernel something to route.
///
/// # Errors
///
/// [`IpcError`] when the source is one this build cannot route, or when there is no plugin engine.
pub fn intent_dispatch(state: &AppState, source: &IntentSourceDto) -> Result<IntentId, IpcError> {
    let started = std::time::Instant::now();
    let routed = match route(state, source) {
        Ok(routed) => routed,
        Err(error) => {
            record_dispatch(state, None, started.elapsed());
            return Err(error);
        }
    };
    record_dispatch(state, Some(&routed), started.elapsed());
    // ★ The window learns an intent exists without asking. The snapshot is read back from the board rather than built
    // from `routed`: the board is what `intent_list` will answer with, and a card assembled here would be a second
    // description of the same intent — the one that disagrees the first time the two are computed differently.
    announce(state, routed.intent_id);
    checkpoint(state, routed.intent_id);
    Ok(routed.intent_id)
}

/// The routing half, split out so the caller can record a duration on both paths.
///
/// # Errors
///
/// As [`intent_dispatch`]: no engine, an unroutable source, or the library's own refusal.
#[allow(clippy::result_large_err)]
fn route(state: &AppState, source: &IntentSourceDto) -> Result<orchestrate::Routed, IpcError> {
    let plugins = plugins(state)?;
    let item = item_for(state, source)?;
    orchestrate::dispatch(plugins, &state.board, &item, synra_store::sqlite::now_ms())
        .map_err(|error| as_ipc(&error))
}

/// Records one dispatch: how long it took, whether routing succeeded, and whether anything offered to handle it.
///
/// # Why the labels are the daemon's
///
/// The daemon has counted `intents_total{outcome=ok|refused}` and `intent_no_handler_total` since its share path was
/// written, and `intent_duration_ms` now in both hosts. A second spelling here (the settle stage, `result=…`) would make
/// the two hosts' snapshots incomparable for the one question these counters answer — "is the share path working", not
/// "which stage did it stop at" (the card already says that).
///
/// # Why a routing failure still gets a duration
///
/// A share the matcher refused still spent its matching time. A distribution built only from successes would answer
/// "how slow is a share" with no failures in it.
fn record_dispatch(
    state: &AppState,
    routed: Option<&orchestrate::Routed>,
    elapsed: std::time::Duration,
) {
    state.metrics.histogram_observe(
        &synra_observe::metrics::SeriesKey::new("intent_duration_ms", []),
        elapsed.as_secs_f64() * 1_000.0,
    );

    let outcome = if routed.is_some() { "ok" } else { "refused" };
    state
        .metrics
        .counter_inc(&synra_observe::metrics::SeriesKey::new(
            "intents_total",
            [("outcome".to_owned(), outcome.to_owned())],
        ));
    // The separate counter is not redundant with `outcome=ok`: an empty plugin set still raises an intent successfully,
    // and "nothing offered to handle this" is the question a user has when nothing opens.
    if routed.is_some_and(|routed| routed.candidates.is_empty()) {
        state
            .metrics
            .counter_inc(&synra_observe::metrics::SeriesKey::new(
                "intent_no_handler_total",
                [],
            ));
    }
}
