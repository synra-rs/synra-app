use super::*;

#[test]
fn recording_consent_tells_the_interface() {
    // ★ The consent command published an event since it was wired, and nothing asserted it. The enable path has a
    // test; this one is the other half — a decision recorded from the interface has to reach the interface, or the
    // card it returns and the list another window draws disagree about whether the user approved anything.
    let (_root, state) = installed();
    let subscriber = state.events.subscribe();

    plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

    let delivery = state.events.take(subscriber).expect("one event");
    assert_eq!(delivery.first_seq(), 1, "the bus numbers it");
    match delivery.events().first() {
        Some(synra_ipc::event::KernelEvent::PluginUpdated(card)) => {
            assert_eq!(card.plugin_id.as_str(), ID);
            assert!(
                !card.granted.is_empty(),
                "the card carries the decision that was just recorded"
            );
        }
        other => panic!("expected a plugin card, got {other:?}"),
    }
}

#[test]
fn granting_every_capability_makes_the_plugin_trusted_and_records_each_decision() {
    // ★ The act the whole table was built for, and the one nothing could perform: `plugin_grants` had a schema, an
    // index, a row type, a reader and **no writer at all**, so the loader took the manifest's own declaration as the
    // grant and a plugin received exactly what it asked for.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent");

    assert_ne!(
        card.state, "awaiting_consent",
        "the plugin is trusted now: {card:?}"
    );
    assert_eq!(
        card.granted,
        vec!["device:list".to_owned(), "log:*".to_owned()]
    );
    assert!(card.denied.is_empty());
    assert_eq!(
        grants(&state),
        vec![("device:list".to_owned(), true), ("log:*".to_owned(), true)],
        "both decisions are on disk"
    );
}

#[test]
fn declining_one_capability_records_the_denial_and_still_trusts() {
    // ★ Two claims in one test, and the second is the one this host used to get wrong.
    //
    // A declined capability is recorded as a **denial**, not omitted: a plugin whose manifest declares two
    // capabilities and whose table holds one row is one the interface cannot describe, so the user is shown a
    // capability they never answered about.
    //
    // And the plugin is **trusted anyway**: it may run with what it was allowed and be refused at the gate for the
    // rest. This host used to require every capability, the daemon required any, and the loader took the rows as
    // the authority — so the same decision produced a plugin that could not run here and one that ran there.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("consent");

    assert_eq!(card.granted, vec!["log:*".to_owned()]);
    assert_eq!(card.denied, vec!["device:list".to_owned()]);
    assert_ne!(
        card.state, "awaiting_consent",
        "one allowed capability is approval"
    );
    assert_eq!(
        grants(&state),
        vec![
            ("device:list".to_owned(), false),
            ("log:*".to_owned(), true)
        ],
    );
}

#[test]
fn denying_every_capability_leaves_it_awaiting_consent() {
    // The other side of the rule: a decision that allows nothing is a refusal, and `should_run` is
    // `enabled && trusted` — so the row stays unable to run even though the user answered the prompt.
    let (_root, state) = installed();

    let card = plugins_consent_respond(&state, ID, &[]).expect("consent");

    assert_eq!(card.state, "awaiting_consent");
    assert!(card.granted.is_empty());
    assert_eq!(card.denied.len(), 2);
}

#[test]
fn changing_a_decision_overwrites_it_rather_than_adding_a_second_answer() {
    // The table's primary key is `(plugin_id, capability)`, so a user who changes their mind must land on one row
    // per capability. Two rows for one capability would make "what did the user decide" unanswerable — and the
    // reader orders by capability, so the two would sit next to each other and look like two different facts.
    let (_root, state) = installed();

    plugins_consent_respond(&state, ID, &["log:*".to_owned()]).expect("a first answer");
    assert_eq!(grants(&state).len(), 2);

    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("a changed answer");

    let decisions = grants(&state);
    assert_eq!(
        decisions.len(),
        2,
        "still one row per capability: {decisions:?}"
    );
    assert!(
        decisions.iter().all(|(_, granted)| *granted),
        "the second answer is the one that stands: {decisions:?}"
    );
}

#[test]
fn a_plugin_whose_manifest_cannot_be_read_is_refused_rather_than_consented_to_blind() {
    // ★ Treating an unreadable manifest as "declares nothing" would record an empty decision set, which is
    // indistinguishable from a user who declined everything — and would mark the plugin trusted while nothing had
    // been shown to anybody.
    let (root, state) = testing::file_state();
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
    drop(root);

    let refused = plugins_consent_respond(&state, ID, &[]);
    assert!(refused.is_err(), "a missing manifest is not a decision");
    assert!(grants(&state).is_empty(), "and nothing was recorded");
}

#[test]
fn the_decision_is_recorded_against_the_manifest_it_was_made_about() {
    // ★ Why `manifest_digest` is a column. A consent decision is about a *document*, and a plugin that ships an
    // update declaring more capabilities is a different document — so the rows that predate it are visibly older
    // than the manifest on disk. That is what makes silent capability growth detectable instead of invisible.
    let (_root, state) = installed();
    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent");
    let before = state.db().plugin_grants(ID).expect("decisions");
    let digest_before = before.first().expect("a row").manifest_digest.clone();
    assert_eq!(digest_before.len(), 64, "sha256 in hex: {digest_before}");

    // The same manifest decided about twice hashes the same, so an unchanged plugin does not look like a changed one.
    plugins_consent_respond(&state, ID, &["log:*".to_owned(), "device:list".to_owned()])
        .expect("consent again");
    let after = state.db().plugin_grants(ID).expect("decisions");
    assert!(
        after.iter().all(|row| row.manifest_digest == digest_before),
        "an unchanged manifest must not look changed: {after:?}"
    );
}

#[test]
fn a_publisher_decision_needs_a_key_this_device_has_met() {
    // ★ "Trusted" is a decision on a row that exists because a signature was verified. A command that could create
    // the row would let the interface trust a key no package ever carried.
    let state = crate::testing::state();

    let fingerprint = "AB12 CD34 EF56 7890";
    let refused = plugins_publisher_decide(&state, "@ghost", fingerprint, "trusted")
        .expect_err("nobody has met it");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);

    state
        .db()
        .record_publisher("@acme", "AB12 CD34 EF56 7890", "a2V5", 1_000)
        .expect("the key was seen");
    let refused = plugins_publisher_decide(&state, "@acme", "1111 2222 3333 4444", "trusted")
        .expect_err("a different key must not be trusted");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
    assert_eq!(plugins_publishers(&state).expect("rows")[0].trust, "seen");
    plugins_publisher_decide(&state, "@acme", fingerprint, "trusted")
        .expect("a known key can be trusted");
    let listed = plugins_publishers(&state).expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].trust, "trusted");

    let refused = plugins_publisher_decide(&state, "@acme", fingerprint, "maybe")
        .expect_err("not a decision");
    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}
