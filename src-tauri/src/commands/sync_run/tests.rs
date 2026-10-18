//! What a finished send is allowed to be written down as.

use super::{Verdict, verdict};
use synra_core::SessionEvent;
use synra_core::messaging::SyncResult;
use synra_protocol::ids::TransferId;

/// The receiver's confirmation that the bytes verified.
fn verified() -> SessionEvent {
    SessionEvent::Verified {
        transfer_id: TransferId::new(),
        digest: "sha256-abc".to_owned(),
    }
}

/// What the receiver said it did with the package.
fn answered(ok: bool, version: Option<&str>, error: Option<&str>) -> SessionEvent {
    SessionEvent::SyncResult {
        result: SyncResult {
            ok,
            installed_version: version.map(str::to_owned),
            activated: true,
            error: error.map(str::to_owned),
        },
    }
}

#[test]
fn an_answer_with_a_version_is_what_gets_recorded() {
    // ★ The version comes from the receiver, not from what this side pushed. A receiver that installed a different
    // version than the one sent (a build that re-pointed the active row) must not be recorded as holding ours.
    let events = [verified(), answered(true, Some("2.0.0"), None)];

    assert_eq!(verdict(&events, "1.2.3"), Verdict::Installed("2.0.0".to_owned()));
}

#[test]
fn a_refusal_records_nothing_and_keeps_its_reason() {
    let events = [verified(), answered(false, None, Some("signature did not cover the bytes"))];

    assert_eq!(
        verdict(&events, "1.2.3"),
        Verdict::Refused("signature did not cover the bytes".to_owned())
    );
}

#[test]
fn an_install_that_names_no_version_is_not_the_pushed_one() {
    // ★ The reply contradicts itself: it says installed and does not say as what. Falling back to the pushed version here
    // would turn "it told me it installed something unnamed" into a fact, and the next plan would skip a send it may need.
    let events = [verified(), answered(true, None, None)];

    assert!(matches!(verdict(&events, "1.2.3"), Verdict::Refused(_)));
}

#[test]
fn no_answer_falls_back_to_the_version_that_was_pushed() {
    // A peer running a build that does not send results still confirmed the bytes. What this side pushed is then the only
    // fact available, and it is a better answer than "nothing happened".
    let events = [verified()];

    assert_eq!(verdict(&events, "1.2.3"), Verdict::Installed("1.2.3".to_owned()));
}

#[test]
fn a_result_without_a_confirmation_is_not_recorded() {
    // ★ The order is the rule. A result is an answer about a transfer the receiver **confirmed**; without `Verified`, the
    // bytes may not have landed at all, and a result arriving anyway is a message about something else.
    let events = [answered(true, Some("1.2.3"), None)];

    assert_eq!(verdict(&events, "1.2.3"), Verdict::Unconfirmed);
}

#[test]
fn nothing_arriving_is_unconfirmed_rather_than_a_quiet_success() {
    assert_eq!(verdict(&[], "1.2.3"), Verdict::Unconfirmed);
}
