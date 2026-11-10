use super::*;

#[test]
fn enabling_a_plugin_tells_the_interface() {
    // ★ The producer the event dispatcher never had. A switch that flipped in the store and nowhere else is what
    // "the shell never emits" looked like from the window: the page showed whatever it had read when it opened.
    let (_root, state) = installed();
    let subscriber = state.events.subscribe();

    super::super::plugins_enable(&state, ID).expect("the plugin is enabled");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(
        delivery.first_seq(),
        1,
        "the bus numbers it, not this command"
    );
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
            assert_eq!(card.plugin_id.as_str(), ID);
            assert!(card.enabled, "the card says what the row now says");
        }
        other => panic!("expected a plugin card, got {other:?}"),
    }
}

#[test]
fn the_shell_loads_an_installed_plugin_rather_than_leaving_its_set_empty() {
    // ★ The whole point of this round: before it, `with_plugins_or_refuse` built an empty `PluginSet` and never
    // loaded anything, so an installed plugin was listed, could be enabled, and was never askable. The assertion is
    // on the set itself, because that is what an intent matches against.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);

    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert_eq!(summary.len(), 1, "refused: {:?}", summary.refused);
    let id = PluginId::parse(ID).expect("an id");
    assert!(state.plugin_set().expect("an engine").lock().contains(&id));

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(card.state, "active");
    assert_eq!(card.state_reason, None, "a running plugin needs no excuse");
}

#[test]
fn safe_mode_is_visible_on_the_card_rather_than_just_an_empty_set() {
    // ★ The M4 acceptance: a plugin that crashes the process three times must not be started a fourth, and the user
    // has to be told why the plugin they can see is not running. Reaching the verdict through three reports is the
    // point — hand-building `SafeMode` would only prove that a made-up value returns false.
    let (root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    let crashes = synra_observe::crash::CrashStore::at(root.path().join("crashes"));
    for offset in 0..synra_observe::crash::SAFE_MODE_THRESHOLD {
        crashes
            .write(&synra_observe::crash::CrashReport::new(
                testing::FIXED_NOW_MS + i64::try_from(offset).expect("a small offset"),
                "0.1.0",
                Some("src/plugin.rs:42".to_owned()),
                Some("the plugin panicked".to_owned()),
                None,
                None,
                None,
            ))
            .expect("the report is written");
    }
    let verdict = crashes.verdict(testing::FIXED_NOW_MS);
    assert!(
        !verdict.allows_plugins(),
        "three crashes did not reach safe mode: {verdict:?}"
    );

    let summary = load_installed_plugins(
        &state,
        &verdict,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert!(
        summary.is_empty(),
        "safe mode loaded something: {:?}",
        summary.loaded
    );
    let state = state.with_plugin_loads(summary);

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(card.state, "unavailable");
    let reason = card
        .state_reason
        .as_deref()
        .expect("safe mode must say why");
    assert!(reason.contains("crashed 3 times"), "{reason}");
}

#[test]
fn a_plugin_that_failed_to_load_says_why_on_its_card() {
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let directory = state.paths.plugin_dir(ID);
    loadable(&state);
    std::fs::remove_file(directory.join("plugin.wasm")).expect("the artifact is removed");

    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    assert_eq!(summary.refused.len(), 1, "{summary:?}");
    let state = state.with_plugin_loads(summary);

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    // ★ A recorded refusal is `faulted`, which the design names and the page already colours red. It used to say
    // `unavailable`, leaving the page's faulted branch unreachable and making a broken plugin look like a build
    // without an engine.
    assert_eq!(card.state, "faulted");
    let reason = card
        .state_reason
        .as_deref()
        .expect("a refusal must say why");
    assert!(reason.contains("artifact"), "{reason}");
}

#[test]
fn a_directory_copied_into_this_shells_data_dir_becomes_a_card() {
    // ★ The shell has its own `plugins/` directory, and before this it never scanned it: a directory an author
    // copied in was invisible in the window while the same directory beside the daemon appeared. Discovery records
    // and does not trust, so the card is `awaiting_consent` rather than running.
    let (_root, state) = testing::file_state();
    write_loadable_files(&state);
    assert!(
        state.db().plugins().expect("rows").is_empty(),
        "nothing is recorded yet"
    );

    let found = discover_plugins(&state, testing::FIXED_NOW_MS);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].plugin_id, ID);
    assert!(found[0].problem.is_none(), "{found:?}");
    let cards = plugins_list(&state).expect("the list");
    let card = cards.first().expect("the discovered plugin");
    // A discovered directory is written disabled and untrusted, and `disabled` is the first word: the user has not
    // switched it on, so the fact that consent has also not been answered is not the card's headline.
    assert_eq!(card.state, "disabled");
    assert!(
        !card.enabled && card.granted.is_empty(),
        "a copied directory is not an approval"
    );
}

#[test]
fn retrying_a_faulted_plugin_loads_it_once_the_cause_is_gone() {
    // ★ The retry path: the row is already enabled and trusted, so the only thing that has to change is the set.
    // This is what the page's Retry button calls, and it must not need a flag to flip.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let directory = state.paths.plugin_dir(ID);
    loadable(&state);
    let artifact = directory.join("plugin.wasm");
    let bytes = std::fs::read(&artifact).expect("the artifact");
    std::fs::remove_file(&artifact).expect("the artifact is removed");
    let summary = load_installed_plugins(
        &state,
        &synra_observe::crash::StartupVerdict::Clean,
        &synra_plugin_api::revocation::RevocationList::empty(),
    );
    let state = state.with_plugin_loads(summary);
    let cards = plugins_list(&state).expect("the list");
    assert_eq!(cards[0].state, "faulted");

    std::fs::write(&artifact, bytes).expect("the artifact is back");
    let card = super::super::plugins_reload(&state, ID).expect("the reload answers with the card");

    assert_eq!(
        card.state, "active",
        "the retry did not load the plugin: {card:?}"
    );
    assert!(
        state
            .plugin_set()
            .expect("an engine")
            .lock()
            .contains(&PluginId::parse(ID).expect("an id")),
        "the card says active but the set is empty"
    );
}

#[test]
fn retrying_a_plugin_that_is_not_installed_is_a_refusal() {
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    let refused = super::super::plugins_reload(&state, ID).expect_err("no row to reload");
    assert_eq!(refused.code, synra_protocol::ErrorCode::PluginNotInstalled);
}

#[test]
fn a_shell_without_an_engine_says_so_on_the_card() {
    // ★ "This build cannot run plugins" and "this plugin is broken" are different answers, and only one of them
    // sends the user to the right place. A shell built without an engine is a real configuration, not a failure.
    let (_root, state) = installed();
    state
        .db()
        .set_plugin_trusted(ID, true, testing::FIXED_NOW_MS)
        .expect("trusted");

    let cards = plugins_list(&state).expect("the list");
    let card = cards.first().expect("the installed plugin");
    assert!(card.state != "active");
    assert_eq!(
        card.state_reason.as_deref(),
        Some("this build has no plugin engine")
    );
}

#[test]
fn enabling_and_disabling_move_the_plugin_in_and_out_of_the_set() {
    // ★ Enable is "activating" in the design's state machine, not a flag to be honoured at the next start. The
    // assertion is on the set because that is what decides whether a share can be answered.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    loadable(&state);
    state
        .db()
        .set_plugin_enabled(ID, false, testing::FIXED_NOW_MS)
        .expect("switched off");
    let id = PluginId::parse(ID).expect("an id");

    super::super::plugins_enable(&state, ID).expect("it enables and loads");
    assert!(
        state.plugin_set().expect("an engine").lock().contains(&id),
        "enable did not load it"
    );

    super::super::plugins_disable(&state, ID).expect("it disables and unloads");
    assert!(
        !state.plugin_set().expect("an engine").lock().contains(&id),
        "disable did not unload it"
    );
}

#[test]
fn confirming_an_enabled_plugins_capabilities_loads_it_without_a_restart() {
    // ★ The consent flow can run after the switch: the user enables a plugin, is asked what it may do, and approves.
    // Before this, the set stayed empty until the next launch and the plugin looked broken for no stated reason.
    let (_root, state) = testing::file_state();
    let state = with_engine(state);
    write_loadable_files(&state);
    state
        .db()
        .insert_plugin(
            ID,
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");

    let card = super::super::plugins_consent_respond(&state, ID, &["log:*".to_owned()])
        .expect("the decision is recorded");
    assert_eq!(
        card.state, "active",
        "the card the consent returns shows the plugin already loaded"
    );
    assert!(
        state
            .plugin_set()
            .expect("an engine")
            .lock()
            .contains(&PluginId::parse(ID).expect("an id"))
    );
}
