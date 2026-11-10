use super::*;

#[test]
fn a_faulted_plugin_is_not_drawn_as_active() {
    // ★ The set holds the bytes and the lifecycle holds the fault. "Loaded" was the whole question before, so a
    // plugin the health ladder had stopped calling still got the green tag of one that was running.
    let row = synra_store::PluginRow {
        plugin_id: ID.to_owned(),
        enabled: true,
        trusted: true,
        health: "faulted".to_owned(),
        installed_at_ms: 0,
        updated_at_ms: 0,
    };
    assert_eq!(super::super::life_state(&row, true, true, true, false), "faulted");
    assert_eq!(super::super::life_state(&row, true, false, true, false), "active");
}

/// Every word `life_state` can return, and nothing else.
///
/// ★ The interface has to name each of these: `messages.plugins.states` is keyed by the word, and a word without
///   an entry falls through to itself — a Chinese card reading `awaiting_consent`. The front end pins the same six
///   in `messages-plugins.test.ts`'s `it.each`, but a list that only names six can only prove those six exist.
///   Nothing on this side said which words there are, so a seventh `return` above used to reach the screen as an
///   English identifier with no test going red.
///
///   This test is the inventory. Its six must equal the six `return`s — that is the one check that can fail when
///   somebody adds a state and forgets the front end.
#[test]
fn life_state_returns_exactly_these_words() {
    let plugin = synra_store::PluginRow {
        plugin_id: ID.to_owned(),
        enabled: true,
        trusted: true,
        health: "ok".to_owned(),
        installed_at_ms: 0,
        updated_at_ms: 0,
    };
    let off = synra_store::PluginRow {
        enabled: false,
        ..plugin.clone()
    };
    let untrusted = synra_store::PluginRow {
        trusted: false,
        ..plugin.clone()
    };

    // Arguments after the row: loaded, faulted, manifest readable, load refused.
    assert_eq!(
        super::super::life_state(&off, true, false, true, false),
        "disabled"
    );
    assert_eq!(
        super::super::life_state(&untrusted, true, false, true, false),
        "awaiting_consent"
    );
    assert_eq!(
        super::super::life_state(&plugin, true, false, false, false),
        "unreadable"
    );
    assert_eq!(
        super::super::life_state(&plugin, true, true, true, false),
        "faulted"
    );
    assert_eq!(
        super::super::life_state(&plugin, true, false, true, false),
        "active"
    );
    assert_eq!(
        super::super::life_state(&plugin, false, false, true, false),
        "unavailable"
    );
}

#[test]
fn a_card_says_whether_the_plugin_has_a_page() {
    // ★ `has_ui` is not `hybrid`: a UI-only plugin has a page and no headless half, and `plugin_ui_open` can open
    // it. The card needs both facts separately, or the one shape with a page but no Wasm would have no way in.
    let (_root, state) = installed();

    let headless_only = plugins_list(&state).expect("the list").remove(0);
    assert!(
        !headless_only.has_ui,
        "a headless-only manifest has no page"
    );
    assert!(!headless_only.hybrid);

    // The same plugin with a UI half written over its manifest. The entry is what `plugin_ui_open` resolves, so a
    // card that reported `has_ui` without one would draw a button that cannot work.
    let manifest = MANIFEST.replace(
        r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } }"#,
        r#""runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false },
                "ui": { "artifact": "dist/ui", "entry": "index.html" } }"#,
    );
    std::fs::write(state.paths.plugin_dir(ID).join("manifest.json"), manifest)
        .expect("a manifest with a page");

    let with_page = plugins_list(&state).expect("the list").remove(0);
    assert!(with_page.has_ui, "the page entry is not reported");
    assert!(with_page.hybrid, "this manifest has both halves");
}

#[test]
fn listing_plugins_sets_the_installed_and_active_gauges() {
    // ★ Set where the truth is read rather than incremented on every write: a write that failed half-way leaves no
    // counter to correct. The fixture installs one enabled plugin; a second, disabled row makes the two numbers
    // differ — an assertion that only checked one gauge would pass even if both were fed the same value.
    use synra_observe::metrics::Value;

    let (_root, state) = installed();
    state
        .db()
        .insert_plugin(
            "@acme/other",
            false,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a second row");

    let _ = plugins_list(&state).expect("the list");

    let snapshot = state.metrics.snapshot();
    assert_eq!(snapshot.get("plugins_installed"), Some(&Value::Gauge(2)));
    assert_eq!(
        snapshot.get("plugins_active"),
        Some(&Value::Gauge(1)),
        "only the enabled one is active"
    );
}

#[test]
fn listing_plugins_reports_the_bytes_each_sandbox_holds() {
    // ★ The storage group's per-plugin gauge: the design names it, and nothing measured it. Nested files are in the
    // fixture because a top-level-only walk would pass a test that only wrote one file at the root.
    use synra_observe::metrics::Value;

    let (_root, state) = installed();
    let sandbox = state.paths.plugin_sandbox_dir(ID);
    std::fs::create_dir_all(sandbox.join("nested")).expect("the sandbox");
    std::fs::write(sandbox.join("cache.bin"), [1u8; 10]).expect("a file");
    std::fs::write(sandbox.join("nested").join("more.bin"), [2u8; 5]).expect("a nested file");

    let _ = plugins_list(&state).expect("the list");

    assert_eq!(
        state
            .metrics
            .snapshot()
            .get("plugin_data_bytes{plugin=@synra-plugin/chat}"),
        Some(&Value::Gauge(15))
    );
}

#[test]
fn a_card_says_what_the_plugin_asks_for() {
    // ★ The field the consent prompt reads. `granted` and `denied` are both empty before the user answers, so a card
    // carrying only those could not say what to ask about — and the prompt would have to read the manifest itself,
    // which is the shell doing the kernel's job.
    let (_root, state) = installed();

    let cards = plugins_list(&state).expect("the list");

    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    assert_eq!(
        card.requested,
        vec!["log:*".to_owned(), "device:list".to_owned()]
    );
    assert!(card.granted.is_empty(), "nothing has been decided yet");
    // ★ The path the removal dialog shows is the directory the removal would delete: both come from
    // `plugin_sandbox_dir`, and this assertion is what keeps the card from inventing a second spelling of it.
    assert_eq!(
        card.data_path,
        state.paths.plugin_sandbox_dir(ID).display().to_string(),
        "the card names the sandbox the removal deletes"
    );
    // A shell with no engine has observed nothing, which is not the same as "healthy".
    assert!(card.health.is_none(), "no engine, no report");
}

#[test]
fn a_loaded_plugins_card_carries_its_health() {
    // ★ The field the contract has always had and the shell never filled: every card said `None`, so a plugin the
    // quota had degraded looked exactly like one that had never run. The report comes from the same budget the gate
    // counts into, so this also proves the card and the gate are looking at one object.
    let (_root, state) = installed();
    let state = with_engine(state);
    {
        let set = state.plugin_set().expect("an engine");
        let mut set = set.lock();
        set.load(synra_core::dispatch::PluginSpec {
            id: PluginId::parse(ID).expect("an id"),
            version: "1.0.0".to_owned(),
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            grants: synra_capability::CapabilitySet::new(),
            policy: synra_plugin_host::InstancePolicy::Stateless,
            limit: 1,
            quota: synra_plugin_host::quota::Quota::default(),
        })
        .expect("loads");
    }

    let cards = plugins_list(&state).expect("the list");
    let card = cards
        .iter()
        .find(|card| card.plugin_id.as_str() == ID)
        .expect("the card");
    let health = card.health.as_ref().expect("a loaded plugin has a report");
    assert_eq!(health.health, "good");
    assert_eq!(health.calls_admitted, 0);
    assert!(!health.usage.is_null(), "the quota in force is carried");
}

#[test]
fn a_plugin_awaiting_consent_is_listed_and_says_so() {
    // The list is where the user finds the plugin to answer about, so the state the consent prompt is triggered by
    // has to be the state the list reports. Asserted through `plugins_list` rather than the consent command's own
    // answer, because the interface reaches this one first.
    let (_root, state) = installed();

    let listed = plugins_list(&state).expect("a list");
    let card = listed.first().expect("the installed plugin");
    assert_eq!(card.plugin_id.as_str(), ID);
    assert_eq!(card.state, "awaiting_consent");
    assert!(
        card.granted.is_empty() && card.denied.is_empty(),
        "nothing has been decided yet"
    );
}
