use std::sync::Arc;

use parking_lot::Mutex;
use synra_plugin_host::host_calls::Host as _;
use synra_protocol::ids::PluginId;

use super::{DesktopHost, fetcher_for, host_of};

/// A plugin identifier, spelled the way the protocol spells it.
fn id(name: &str) -> PluginId {
    PluginId::parse(name).expect("a plugin id")
}

fn host() -> DesktopHost {
    let state = crate::testing::state();
    // ★ The plugin rows have to exist: `plugin_storage` references `plugins`, so a value stored against a plugin this
    // database has never heard of is refused by the schema. That is the cascade doing its job — and it is why the first
    // version of this test failed with `FOREIGN KEY constraint failed`, which is a better answer than silently keeping
    // storage for a plugin nobody installed.
    for plugin in ["@acme/notes", "@acme/chat"] {
        state
            .db
            .lock()
            .insert_plugin(plugin, true, true, "healthy", 1, 1)
            .expect("a plugin row");
    }
    DesktopHost::new(std::sync::Arc::clone(&state.db))
}

#[test]
fn a_capability_names_a_host_or_it_names_nothing() {
    // The parsing rule on its own, because the tests below depend on it being the fail-closed reading.
    assert_eq!(
        host_of("network:fetch:api.github.com").as_deref(),
        Some("api.github.com")
    );
    // A method qualifier is not part of the host: the guard decides *where*, and the method travels as sent.
    assert_eq!(
        host_of("network:fetch:api.github.com:POST").as_deref(),
        Some("api.github.com")
    );

    // ★ `*` is the family spelling the **gate** uses, not a hostname — reading it as one builds a guard for a machine
    // called `*`, and reading it as "anything" is the capability an SSRF guard exists to make deliberate.
    assert_eq!(host_of("network:fetch:*"), None);
    assert_eq!(host_of("network:fetch:"), None);
    assert_eq!(host_of("network:fetch"), None);
    assert_eq!(host_of("fs:read:/data/**"), None);
}

#[test]
fn a_plugin_with_no_host_grant_is_told_which_spelling_it_needs() {
    // Nothing granted: `capabilityDenied`, because the answer is "ask the user" rather than "that host is not one of
    // them" — and a plugin author can act on the first.
    let host = host_with(&[]);
    let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("no grant");

    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED
    );
    assert!(
        refused.message.contains("network:fetch:"),
        "and it says what to ask for: {}",
        refused.message
    );
}

#[test]
fn a_family_grant_with_a_star_is_not_a_grant_to_reach_everything() {
    // ★ The whole point of the guard, at the shell: `network:fetch:*` passes the **gate** — it is the family — and it
    // must not become "every host on the internet".
    let host = host_with(&[("network:fetch:*", true)]);
    let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("no host named");

    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED,
        "{refused}"
    );
}

#[test]
fn a_granted_host_gives_a_client_and_withdrawing_it_takes_the_client_away() {
    // ★ The test the cache needs: a client built for a grant must not outlive the grant. If the cache were keyed on the
    // plugin alone, the call after the withdrawal would hand back the earlier client — and a user who withdrew a host
    // would keep reaching it.
    let host = host_with(&[("network:fetch:api.github.com", true)]);
    assert!(fetcher_for(&host, &id("@acme/notes")).is_ok(), "granted");
    assert!(fetcher_for(&host, &id("@acme/notes")).is_ok(), "and again");

    host.db
        .lock()
        .record_grants(
            "@acme/notes",
            &[("network:fetch:api.github.com".to_owned(), false)],
            2,
            "d2",
        )
        .expect("the withdrawal is recorded");
    let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("withdrawn");

    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED,
        "{refused}"
    );
}

#[test]
fn a_plugin_can_store_something_in_the_shell_too() {
    // ★ The same five functions the daemon implements, over the same table.
    //
    // They refused with `notImplemented` here for as long as the store had no table — and a plugin that behaves
    // differently depending on which host runs it is one its author cannot test. The shell has a database, so there
    // was never a reason for it to be the one host where nothing persists.
    //
    // The round trip, because a store that accepts a write and returns nothing is indistinguishable from one that
    // discarded it.
    let host = host();
    let plugin = id("@acme/notes");

    assert_eq!(
        host.storage_get(&plugin, "k").expect("reads"),
        None,
        "an empty store reads as absent"
    );

    host.storage_set(&plugin, "k", "kept").expect("writes");
    assert_eq!(
        host.storage_get(&plugin, "k").expect("reads").as_deref(),
        Some("kept")
    );
    assert_eq!(
        host.storage_used(&plugin).expect("measures"),
        4,
        "the value's bytes"
    );
    assert_eq!(
        host.storage_keys(&plugin, None).expect("lists"),
        vec!["k".to_owned()]
    );

    // And another plugin's key space is its own, which is what the pair being the primary key buys.
    let other = id("@acme/chat");
    assert_eq!(host.storage_get(&other, "k").expect("reads"), None);

    host.storage_delete(&plugin, "k").expect("removes");
    assert_eq!(host.storage_used(&plugin).expect("measures"), 0);
}

pub(super) fn host_with(grants: &[(&str, bool)]) -> DesktopHost {
    let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
    db.migrate().expect("migrates");
    db.insert_plugin("@acme/notes", true, true, "healthy", 1, 1)
        .expect("a plugin row");
    let decisions: Vec<(String, bool)> = grants
        .iter()
        .map(|(cap, granted)| ((*cap).to_owned(), *granted))
        .collect();
    db.record_grants("@acme/notes", &decisions, 1, "digest")
        .expect("decisions");
    DesktopHost::new(Arc::new(Mutex::new(db)))
}

#[test]
fn the_shell_is_a_desktop_rather_than_a_headless_build() {
    // ★ The whole reason this host exists rather than the daemon's.
    //
    // A shell has a desktop, so `host_kind` says so and `platform:*` reaches the real implementation instead of
    // refusing. The daemon answers `headless` to the same question, and a plugin that reads it can tell which machine
    // it is on.
    let host = host_with(&[]);

    assert_eq!(host.host_kind(), "desktop");
    // ★ And it names what this machine can do.
    //
    // The daemon answers with an empty list, because it can do none of it. This one asks `synra-platform` for the
    // actions the running system supports, which is the difference between "no desktop" and "a desktop that cannot
    // simulate keys".
    assert!(
        !host.platform_capabilities().is_empty(),
        "a desktop supports something"
    );
    assert!(
        host.platform_capabilities()
            .iter()
            .any(|name| name.starts_with("platform:")),
        "{:?}",
        host.platform_capabilities()
    );
}

#[test]
fn a_bad_address_is_the_plugins_mistake_rather_than_a_missing_feature() {
    // ★ The guard runs before anything is launched, and the code says whose problem it is.
    //
    // `javascript:` is a value the plugin supplied, so the refusal is `badArgument` — an author can fix that. A
    // refusal shaped like "this shell cannot open a browser" would send them looking at the installation instead.
    let host = host_with(&[]);
    let plugin = id("@acme/notes");

    let refused = host
        .platform(
            &plugin,
            super::PlatformAction::OpenUrl,
            "javascript:alert(1)",
        )
        .expect_err("the guard refuses this scheme");
    assert_eq!(refused.code, "badArgument", "{refused:?}");
}

#[test]
fn can_answers_from_what_the_user_actually_granted() {
    // The same reading the daemon makes, for the same reason: the answer a plugin gets about itself has to agree
    // with the gate that will judge it.
    let host = host_with(&[("log:*", true), ("device:list", false)]);

    assert!(host.can(&id("@acme/notes"), "log:info"));
    assert!(!host.can(&id("@acme/notes"), "device:list"));
    assert!(!host.can(&id("@acme/other"), "log:info"));
    assert!(!host.can(&id("@acme/notes"), "not a capability"));
}

#[test]
fn the_groups_this_shell_does_not_implement_refuse_with_one_code() {
    // A plugin branches on the code, and `notImplemented` says "this shell cannot do that" rather than "you are not
    // allowed" — which is the difference between hiding a feature and hunting for a permission already granted.
    let host = host_with(&[]);
    let plugin = id("@acme/notes");

    {
        let error = host
            .device_send(&plugin, "d", "x", "{}")
            .expect_err("this shell does not implement it");
        // ★ Read from the contract rather than spelled here. This assertion used to compare against the
        // literal `"notImplemented"`, which is a string this test and the implementation could agree on while both were
        // wrong — the shape of assertion that cannot catch the implementation spelling it wrong.
        assert_eq!(
            error.code,
            synra_plugin_host::host_calls::CallError::NOT_IMPLEMENTED,
            "{error:?}"
        );
    }

    // ★ Storage is **not** in that list any more, and its absence is the point.
    //
    // It was the largest group in it and the shell is where a plugin is most likely to run. Asserted positively, so
    // that removing it from the loop cannot be mistaken for removing the feature.
    let plugin = id("@acme/notes");
    host.storage_set(&plugin, "k", "v").expect("writes");
    assert_eq!(
        host.storage_get(&plugin, "k").expect("reads").as_deref(),
        Some("v")
    );
    assert_eq!(host.storage_used(&plugin).expect("measures"), 1);

    // ★ And network is not in that list either. Asserted positively the same way: the refusal below is a **capability**
    // answer rather than a "this shell cannot" one, which is the difference this test exists to protect — and it is
    // reachable without a network, because the guard refuses before anything is sent.
    let refused = host
        .fetch(&plugin, r#"{"url":"https://api.github.com/"}"#)
        .expect_err("no grant");
    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED,
        "{refused:?}"
    );

    // ★ Actions are out of the list too, in the half that belongs to this host: a plugin declares its own and lists
    // them. The declaration is validated here — a schema that is not JSON is the plugin's error, not a row every
    // reader has to guard against.
    host.action_declare(&plugin, "send", r#"{"type":"object"}"#)
        .expect("declares");
    let listed = host.action_list(&plugin).expect("lists");
    assert!(
        listed.contains("\"send\""),
        "the declaration did not reach the list: {listed}"
    );
    let refused = host
        .action_declare(&plugin, "broken", "not json")
        .expect_err("not JSON");
    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::BAD_ARGUMENT,
        "{refused:?}"
    );

    // The one action call that still refuses is **another** plugin's: that is a cross-plugin permission, and the
    // path it needs — resolve the target, check the grant, reach a set this host does not own — is not the one the
    // page's own `self.invoke` uses.
    let refused = host
        .action_invoke(&plugin, "@acme/other", "send", "null")
        .expect_err("this host does not reach another plugin");
    assert_eq!(
        refused.code,
        synra_plugin_host::host_calls::CallError::NOT_IMPLEMENTED,
        "{refused:?}"
    );
}

#[test]
fn an_audited_call_is_written_down_rather_than_only_remembered() {
    // ★ The half the shell was missing. The ring is written by the kernel and read by the running process; this is what
    // makes the trail outlive it. What is asserted is the **table**, read back through the store rather than the entry
    // that was handed in — so a sink that accepted the entry and wrote nothing fails here.
    let db = Arc::new(Mutex::new({
        let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
        // The migrations are not run by `open_in_memory`, and without them the table does not exist — which is what the
        // first version of this test found, as `no such table: plugin_audit` rather than as a passing assertion.
        db.migrate().expect("the schema");
        db
    }));
    let sink = super::audit_sink(Arc::clone(&db));

    let entry = synra_observe::audit::AuditEntry {
        plugin: PluginId::parse("@acme/notes").expect("an id"),
        capability: "device:list".parse().expect("a capability"),
        outcome: synra_observe::audit::AuditOutcome::Allowed,
        at_ms: 1_000,
        elapsed_ms: Some(3),
        trace: None,
    };
    sink(&entry);

    let trail = db.lock().audit_trail().expect("the trail");
    assert_eq!(trail.len(), 1, "the record reached the table");
    assert_eq!(trail[0].plugin_id, "@acme/notes");
    assert_eq!(trail[0].capability, "device:list");
    assert_eq!(trail[0].outcome, "allowed");
    assert_eq!(trail[0].elapsed_ms, Some(3));
}

#[test]
fn the_host_reports_the_interfaces_locale_and_theme() {
    // ★ The two settings `activate(config)` and `env.locale()` answer from. They used to be hardcoded to empty and
    // false, so a plugin was told the interface was in no language and light while the window was `zh-CN` and dark.
    let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
    db.migrate().expect("the schema");
    db.set_config("ui.locale", &serde_json::json!("zh-CN"), 1_000)
        .expect("the locale");
    db.set_config("ui.theme", &serde_json::json!("dark"), 1_000)
        .expect("the theme");
    let host = DesktopHost::new(Arc::new(Mutex::new(db)));

    assert_eq!(host.locale(), "zh-CN");
    assert!(
        host.dark_mode(),
        "the interface setting is dark and the host said light"
    );
}
