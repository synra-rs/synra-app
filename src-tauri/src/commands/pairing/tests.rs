//! What pairing refuses, and what it names.

use super::{platform_name, platform_of};
use synra_protocol::session::PlatformKind;

#[test]
fn a_platform_survives_the_round_trip_between_wire_and_column() {
    // ★ The column and the wire have separate vocabularies, and a value that lost its name between them would be a device the
    // interface shows as "unknown" on both screens — which is exactly what a user cannot check a code against.
    for platform in [PlatformKind::Desktop, PlatformKind::Android, PlatformKind::Ios] {
        assert_eq!(platform_of(platform_name(platform)), platform);
    }
}

#[test]
fn the_strings_this_device_stores_for_a_platform() {
    // Spelled out rather than derived, because these are the strings already written into every existing database — changing the
    // spelling would be a migration, not a rename.
    assert_eq!(platform_name(PlatformKind::Desktop), "desktop");
    assert_eq!(platform_name(PlatformKind::Android), "android");
    assert_eq!(platform_name(PlatformKind::Ios), "ios");
}

#[test]
fn answering_a_request_that_is_not_waiting_is_refused() {
    // ★ The refusal a stale screen reaches: the prompt expired, or another window answered it. Answering "done" would tell the
    // interface a pairing happened when nothing did.
    let state = crate::testing::state();
    let absent = synra_protocol::ids::RequestId::new().to_string();

    let refused = super::pairing_respond(&state, &absent, true).expect_err("nothing is waiting");

    assert_eq!(refused.code, synra_protocol::ErrorCode::NotPaired);
}

#[test]
fn asking_a_device_the_scan_never_saw_is_refused_by_name() {
    let state = crate::testing::state();
    let absent = synra_protocol::ids::DeviceId::new().to_string();

    let refused = super::pairing_request(&state, &absent).expect_err("nothing was seen");

    assert_eq!(refused.code, synra_protocol::ErrorCode::NotPaired);
}

#[test]
fn an_identifier_that_does_not_parse_is_the_callers_fault() {
    let state = crate::testing::state();

    let refused = super::pairing_request(&state, "not-a-device-id").expect_err("it does not parse");

    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}
