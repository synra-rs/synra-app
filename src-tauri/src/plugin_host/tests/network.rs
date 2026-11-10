use super::super::{fetcher_for, host_of};
use super::{host_with, id};

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
