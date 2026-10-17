//! What the transfer registry does with what a worker reports.

use super::Transfers;
use synra_ipc::dto::TransferSnapshot;
use synra_protocol::ids::{DeviceId, TransferId};

/// A snapshot a worker might report, with every field named.
fn snapshot(transfer_id: TransferId, state: &str) -> TransferSnapshot {
    TransferSnapshot {
        transfer_id,
        peer_device: DeviceId::new(),
        peer_name: "Study PC".to_owned(),
        direction: "send".to_owned(),
        kind: "plugin-bundle".to_owned(),
        state: state.to_owned(),
        file_name: "bundle.zip".to_owned(),
        received_bytes: 0,
        total_bytes: 1_024,
        blocks_done: 0,
        blocks_total: 1,
        abort_reason: None,
        started_at_ms: 1_000,
    }
}

#[test]
fn cancelling_a_transfer_that_is_not_in_flight_is_refused() {
    // ★ A cancel naming a transfer nobody is running means the panel is out of date. Answering "done" would leave it
    // believing it stopped something, so the refusal names the identifier and the panel can re-read.
    let state = crate::testing::state();

    let refused =
        super::transfer_cancel(&state, "00000000-0000-4000-8000-000000000003").expect_err("nothing is in flight");

    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);
}

#[test]
fn cancelling_a_transfer_that_is_in_flight_drops_it() {
    let state = crate::testing::state();
    let id = TransferId::new();
    state.transfers.record(snapshot(id, "active"));
    let subscriber = state.events.subscribe();

    super::transfer_cancel(&state, &id.to_string()).expect("it is in flight");

    assert!(state.transfers.is_empty(), "the registry no longer holds it");
    assert!(super::transfer_list(&state).is_empty(), "and the list does not show it");
    // ★ And the panel is told, or the row stays until the page is reopened.
    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::TransferFinished { transfer_id }] => assert_eq!(*transfer_id, id),
        other => panic!("expected a finished transfer, got {other:?}"),
    }
}

#[test]
fn cancelling_something_that_is_not_an_identifier_is_the_callers_fault() {
    let state = crate::testing::state();

    let refused = super::transfer_cancel(&state, "not-an-id").expect_err("it does not parse");

    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}

#[test]
fn sending_text_to_a_device_that_was_never_paired_is_refused_and_writes_nothing() {
    // ★ The temporary file is written **after** the address is known, so a refusal must leave the temp directory clean: a
    // command that wrote the file first would leave one behind for every mistyped device id.
    let state = crate::testing::state();
    let before = temp_files();
    let request = synra_ipc::dto::SendTextRequest {
        to: DeviceId::new(),
        text: "hello".to_owned(),
        file_name: "hello.txt".to_owned(),
        context_id: None,
    };

    let refused = super::transfer_send_text(&state, &request).expect_err("nothing to dial");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    assert_eq!(temp_files(), before, "no temporary file was written");
    assert!(state.transfers.is_empty(), "and no row");
}

/// How many `message-*.txt` files the temporary directory holds, which is what this command would add.
fn temp_files() -> usize {
    std::fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("message-"))
                .count()
        })
        .unwrap_or(0)
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
            link_token: "unused-here".to_owned(),
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

    let refused = super::transfer_send_file(&state, &request).expect_err("nothing answers");

    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);
    assert!(state.transfers.is_empty(), "the row was removed on the failure path");

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

    let refused = super::transfer_send_file(&state, &request).expect_err("nothing to dial");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    assert!(state.transfers.is_empty(), "a refused send leaves no row behind");
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

    let peer = super::local_peer(&identity);

    assert_eq!(peer.device_id, identity.device_id);
    assert_eq!(peer.display_name, "Study PC");
    assert_eq!(peer.platform, synra_protocol::session::PlatformKind::Desktop);
    assert!(!peer.features.is_empty(), "a build announces what it can do");
}

#[test]
fn a_sender_becomes_the_row_the_panel_shows() {
    // ★ The two things the worker must not get wrong: the peer comes from the link (not from a request that has none), and the
    // block count is the sender's own arithmetic rather than a guess at what the receiver will report.
    use synra_protocol::ids::PluginId;
    use synra_protocol::session::{TransferKind, TransferMeta, TransferRequest};

    let request = TransferRequest {
        transfer_id: TransferId::new(),
        kind: TransferKind::PluginBundle,
        byte_length: 5_000,
        chunk_size: 1_000,
        digest: synra_protocol::session::SriDigest::new(synra_protocol::session::DigestAlgorithm::Sha256, "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU")
            .expect("a digest"),
        sync_session_id: None,
        meta: TransferMeta::PluginBundle {
            plugin_id: PluginId::parse("@acme/hello").expect("a plugin id"),
            version: "1.2.3".to_owned(),
        },
    };
    let peer = (DeviceId::new(), "Study PC");

    let snapshot = super::snapshot_from(&request, peer, "active", 1_000);

    assert_eq!(snapshot.peer_name, "Study PC", "the name is the link's, not the request's");
    assert_eq!(snapshot.kind, "plugin-bundle");
    assert_eq!(snapshot.file_name, "@acme/hello 1.2.3", "what the row names");
    assert_eq!(snapshot.total_bytes, 5_000);
    assert_eq!(snapshot.blocks_total, 5, "five chunks of a thousand bytes");
    assert_eq!(snapshot.blocks_done, 0, "nothing sent yet");
}

#[test]
fn an_attachment_is_named_by_its_file() {
    use synra_protocol::session::{TransferKind, TransferMeta, TransferRequest};

    let request = TransferRequest {
        transfer_id: TransferId::new(),
        kind: TransferKind::Attachment,
        byte_length: 10,
        chunk_size: 1_000,
        digest: synra_protocol::session::SriDigest::new(synra_protocol::session::DigestAlgorithm::Sha256, "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU")
            .expect("a digest"),
        sync_session_id: None,
        meta: TransferMeta::Attachment {
            file_name: "notes.txt".to_owned(),
            mime_type: None,
            context_id: None,
        },
    };

    let snapshot = super::snapshot_from(&request, (DeviceId::new(), "Laptop"), "queued", 0);

    assert_eq!(snapshot.kind, "attachment");
    assert_eq!(snapshot.file_name, "notes.txt");
    assert_eq!(snapshot.blocks_total, 1, "a small file is still one block");
}

#[test]
fn announcing_a_transfer_records_it_and_tells_the_interface() {
    // ★ The two halves the worker must not forget separately: a snapshot recorded but not announced is a panel that is right
    // the next time it opens and wrong until then.
    let state = crate::testing::state();
    let id = TransferId::new();
    let subscriber = state.events.subscribe();

    super::announce_transfer(&state, snapshot(id, "active"));

    assert_eq!(super::transfer_list(&state).len(), 1, "the registry has it");
    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::TransferUpdated(transfer)] => assert_eq!(transfer.transfer_id, id),
        other => panic!("expected a transfer update, got {other:?}"),
    }
}

#[test]
fn a_recorded_transfer_is_listed_and_a_forgotten_one_is_not() {
    let transfers = Transfers::new();
    assert!(transfers.is_empty());

    let id = TransferId::new();
    transfers.record(snapshot(id, "active"));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.list().len(), 1);

    transfers.forget(&id);
    assert!(transfers.is_empty(), "a finished transfer is not in flight");
}

#[test]
fn a_second_report_replaces_the_first_rather_than_joining_it() {
    // ★ Replacement, not merge: a field the worker stops reporting must not keep its old value, or the progress bar freezes
    // while the transfer moves.
    let transfers = Transfers::new();
    let id = TransferId::new();

    transfers.record(snapshot(id, "active"));
    transfers.record(snapshot(id, "finishing"));

    let listed = transfers.list();
    assert_eq!(listed.len(), 1, "one transfer, not two");
    assert_eq!(listed[0].state, "finishing");
}

#[test]
fn the_list_is_ordered_by_identifier_not_by_arrival() {
    // ★ Otherwise the order would be the network's, and the same two transfers would swap places between two reads — which
    // the interface draws as two rows moving for no reason.
    let transfers = Transfers::new();
    let one = TransferId::new();
    let two = TransferId::new();
    transfers.record(snapshot(one, "active"));
    transfers.record(snapshot(two, "active"));

    let ordered: Vec<TransferId> = transfers.list().into_iter().map(|t| t.transfer_id).collect();

    let (first, second) = if one < two { (one, two) } else { (two, one) };
    assert_eq!(ordered, vec![first, second], "sorted by identifier");
}
