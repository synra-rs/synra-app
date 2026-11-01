//! What the pairing commands refuse.
//!
//! # What this file no longer covers, and why
//!
//! It used to drive a whole first pairing through `pairing_invite` / `pairing_listen` / `pairing_connect` — the
//! invitation flow, where a person carried certificates between two screens. Those commands are gone: pairing now dials
//! the address the devices screen already knows, and the two devices compare six digits. A test of the old commands would
//! be a test of code that no longer exists.
//!
//! What can still be tested here without two machines is the refusal half: a request naming something that is not a
//! device, or a device this machine has no address for, must not open a socket. The dial itself needs a peer listening on
//! a real port, and its happy path belongs to the protocol crate rather than to these thin commands.

use super::*;

/// A request for a device this machine has never heard must be refused before anything is dialled.
///
/// # Why this is the interesting case
///
/// The address comes from the row the devices screen read. A stale identifier — a device that went away, or one the
/// window was never told about — would otherwise reach `TcpStream::connect` with an address parsed out of nothing, and the
/// person would be told the connection failed rather than that the device is not there.
#[test]
fn an_addressless_device_is_refused_before_the_socket_is_opened() {
    let state = crate::testing::state();
    let unknown = DeviceId::new().to_string();
    let refused = pairing_request(&state, &unknown).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
}

/// The identifiers are parsed before anything else is read.
#[test]
fn malformed_identifiers_are_refused() {
    let state = crate::testing::state();
    assert_eq!(
        pairing_request(&state, "not-a-device").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        pairing_cancel(&state, "invalid").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        answer_pairing(&state, "invalid", true).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}
