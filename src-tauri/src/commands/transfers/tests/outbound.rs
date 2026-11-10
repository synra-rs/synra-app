use super::*;

#[test]
fn sending_text_to_a_device_that_was_never_paired_is_refused_and_writes_nothing() {
    // ★ The temporary file is written **after** the address is known, so a refusal must leave the outbox empty: a command that
    // wrote the file first would leave one behind for every mistyped device id.
    let state = crate::testing::state();
    let request = synra_ipc::dto::SendTextRequest {
        to: DeviceId::new(),
        text: "hello".to_owned(),
        file_name: "hello.txt".to_owned(),
        context_id: None,
    };

    let refused = super::super::transfer_send_text(&state, &request).expect_err("nothing to dial");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    // ★ The outbox is under the app's own cache directory, not the system one: a file holding what a person typed must not sit
    // where every other process on the machine can read it.
    let outbox = state.paths.cache_dir().join("outbox");
    assert!(
        !outbox.exists(),
        "no outbox was created: {}",
        outbox.display()
    );
    assert!(state.transfers.is_empty(), "and no row");
}

#[test]
fn a_send_to_a_paired_device_with_a_dead_address_fails_and_leaves_no_row() {
    // ★ The path a real broken network takes: the device is paired and has an address, and nothing answers there. The command
    // must answer `TransferFailed` **and** remove the row it recorded — a failed send that stayed in the registry would look
    // like one still running.
    let state = crate::testing::state();
    let device = DeviceId::new();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: device.to_string(),
            display_name: "Study PC".to_owned(),
            platform: "windows".to_owned(),
            features: "[]".to_owned(),
            // Nothing listens on port 1, so the dial is refused rather than hanging.
            address: Some("127.0.0.1:1".to_owned()),
            paired_at_ms: 1_000,
        })
        .expect("a paired device");

    // A real file: the command opens the sender before it dials, so a path that does not exist is refused as a bad argument
    // and never reaches the dial this test is about.
    let path = std::env::temp_dir().join(format!("synra-send-{}.bin", std::process::id()));
    std::fs::write(&path, b"x").expect("a file to send");

    let request = synra_ipc::dto::SendFileRequest {
        to: device,
        path: path.to_string_lossy().into_owned(),
        context_id: None,
    };

    let refused = super::super::transfer_send_file(&state, &request).expect_err("nothing answers");

    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);
    assert!(
        state.transfers.is_empty(),
        "the row was removed on the failure path"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn sending_to_a_device_that_was_never_paired_is_a_refusal_that_says_why() {
    // ★ The address comes from `paired_devices`, so a device nobody paired has none — and the answer must be "connect it
    // first", not "the transfer failed". This is also the whole command's reach without a peer: everything after the address
    // needs a socket.
    let state = crate::testing::state();
    let request = synra_ipc::dto::SendFileRequest {
        to: DeviceId::new(),
        path: "does-not-matter.bin".to_owned(),
        context_id: None,
    };

    let refused = super::super::transfer_send_file(&state, &request).expect_err("nothing to dial");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    assert!(
        state.transfers.is_empty(),
        "a refused send leaves no row behind"
    );
}

#[test]
fn what_this_shell_says_when_it_dials_is_the_identity() {
    // ★ The name is the one discovery already announces — two sources would let one device appear twice on a network — and
    // the platform has to become the wire's enum rather than a string a peer would decline.
    let identity = synra_core::identity::Identity {
        device_id: DeviceId::new(),
        display_name: "Study PC".to_owned(),
        platform: "windows".to_owned(),
        created_at_ms: 0,
    };

    let peer = super::super::local_peer(&identity);

    assert_eq!(peer.device_id, identity.device_id);
    assert_eq!(peer.display_name, "Study PC");
    assert_eq!(
        peer.platform,
        synra_protocol::session::PlatformKind::Desktop
    );
    assert!(
        !peer.features.is_empty(),
        "a build announces what it can do"
    );
}

#[test]
fn a_text_send_is_named_after_the_request_rather_than_the_clock() {
    // ★ The receiver names the arriving file after this path. `message-<ms>.txt` threw away the only part a person
    // recognises; the stem now comes from the request, and a stem with a separator in it stays a name rather than a path.
    let outbox = std::path::Path::new("/outbox");

    let named = super::super::outbox_path(outbox, "notes.txt", 42);
    assert_eq!(
        named.file_name().and_then(std::ffi::OsStr::to_str),
        Some("notes-42.txt")
    );

    let escaped = super::super::outbox_path(outbox, "../../evil.txt", 42);
    assert_eq!(
        escaped.file_name().and_then(std::ffi::OsStr::to_str),
        Some("evil-42.txt")
    );
    assert_eq!(
        escaped.parent(),
        Some(outbox),
        "the name escaped the outbox"
    );

    let anonymous = super::super::outbox_path(outbox, "..", 42);
    assert_eq!(
        anonymous.file_name().and_then(std::ffi::OsStr::to_str),
        Some("message-42.txt")
    );
}
