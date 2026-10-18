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
fn a_device_the_user_accepts_forever_is_not_asked_again() {
    // ★ The preference is per device and persisted under one configuration key: a fresh install asks, an accepted device
    // does not, and a different device still does.
    let state = crate::testing::state();
    let remembered = "11111111-1111-4111-8111-111111111111";
    let other = "22222222-2222-4222-8222-222222222222";

    assert!(!super::auto_accept_peer(&state, remembered), "a fresh install asks");
    super::remember_peer(&state, remembered);
    assert!(super::auto_accept_peer(&state, remembered), "the answer was remembered");
    assert!(!super::auto_accept_peer(&state, other), "another device still asks");

    // Read back through the settings snapshot, which is what a screen would draw.
    let config = crate::commands::config::config_get(&state).expect("the configuration reads");
    let accepted = config
        .values
        .get("transfers.acceptFrom")
        .and_then(|value| value.as_array())
        .expect("the preference is stored");
    assert_eq!(accepted, &vec![serde_json::json!(remembered)]);
}

#[test]
fn forgetting_a_device_forgets_that_it_was_trusted() {
    // ★ The preference is about a device; once the device is forgotten, the preference is about nothing. It is also
    // removed rather than left as an empty array, so a settings screen has nothing to interpret.
    let state = crate::testing::state();
    let peer = "11111111-1111-4111-8111-111111111111";

    super::remember_peer(&state, peer);
    assert!(super::auto_accept_peer(&state, peer));
    super::forget_sender(&state, peer);

    assert!(!super::auto_accept_peer(&state, peer), "the device asks again");
    let config = crate::commands::config::config_get(&state).expect("the configuration reads");
    assert!(
        config.values.get("transfers.acceptFrom").is_none(),
        "the empty preference was removed rather than stored"
    );
}

#[test]
fn answering_a_prompt_that_is_not_waiting_is_refused() {
    // ★ The prompt's id is one a panel was told about. Answering twice, or answering after the window closed, is an
    // answer to a question that no longer exists — and a kernel that said "done" would leave the panel believing it
    // settled something a peer is still waiting on.
    let state = crate::testing::state();
    let refused = super::transfer_respond(&state, "00000000-0000-4000-8000-000000000003", true, None)
        .expect_err("nothing is waiting");
    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);

    let invalid = super::transfer_respond(&state, "not-an-id", true, None).expect_err("it does not parse");
    assert_eq!(invalid.code, synra_protocol::ErrorCode::InvalidArgument);
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
fn a_send_over_a_connected_link_reports_every_block_and_finishes() {
    // ★ The success path, without a socket: `send_ready` is everything after the dial, so a memory link drives it exactly as a
    // socket would. Two claims: the peer received every block in order, and the panel was told about each one and then about
    // the end — the two events the interface needs and cannot derive from a snapshot.
    use crate::testing::state as app_state;
    use synra_core::link::memory::MemoryStream;
    use synra_core::peer::Flags;
    use synra_protocol::handshake::Agreed;

    let state = app_state();
    let (near, far) = MemoryStream::pair_with_capacity(4 * 1024 * 1024);
    let agreed = Agreed { common: Vec::new(), missing_required: Vec::new() };
    let local = DeviceId::new();
    let peer = DeviceId::new();
    let mut sending = synra_core::link::PeerLink::new(near, local, peer, agreed.clone(), Flags::no_timeout());
    let mut receiving = synra_core::link::PeerLink::new(far, peer, local, agreed, Flags::no_timeout());

    let path = std::env::temp_dir().join(format!("synra-ready-{}.bin", std::process::id()));
    std::fs::write(&path, vec![3u8; 2 * synra_core::transfer::DEFAULT_CHUNK_SIZE as usize]).expect("a file");
    let sender =
        synra_core::transfer::TransferSender::open(&path, synra_core::transfer::DEFAULT_CHUNK_SIZE).expect("a sender");
    let planned = sender.request();
    let total = u32::try_from(planned.byte_length.div_ceil(u64::from(planned.chunk_size))).expect("blocks");
    let row = super::snapshot_from(&planned, (peer, "Study PC"), "queued", 0);
    let subscriber = state.events.subscribe();

    super::send_ready(&state, peer, &mut sending, sender, row).expect("it sends");

    for expected in 0..total {
        match receiving.receive_inbound().expect("a block") {
            synra_core::link::Inbound::Block { chunk_id, .. } => {
                assert_eq!(chunk_id, expected, "blocks arrive in order");
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }
    let delivery = state.events.take(subscriber).expect("announced");
    assert!(
        delivery
            .events()
            .iter()
            .any(|event| matches!(event, synra_ipc::event::KernelEvent::TransferFinished { .. })),
        "the end was announced"
    );
    assert!(state.transfers.is_empty(), "and the row is gone");

    let _ = std::fs::remove_file(path);
}

#[test]
fn a_cancelled_send_stops_before_the_first_block_and_tells_the_peer() {
    // ★ The token is checked in the block loop, so a cancel is not "close the window and hope": the peer receives an
    // abort frame, and the panel's row is removed by the same exit path as a completed send.
    use crate::testing::state as app_state;
    use synra_core::link::memory::MemoryStream;
    use synra_core::peer::Flags;
    use synra_protocol::handshake::Agreed;

    let state = app_state();
    let (near, far) = MemoryStream::pair_with_capacity(4 * 1024 * 1024);
    let agreed = Agreed { common: Vec::new(), missing_required: Vec::new() };
    let local = DeviceId::new();
    let peer = DeviceId::new();
    let mut sending = synra_core::link::PeerLink::new(near, local, peer, agreed.clone(), Flags::no_timeout());
    let mut receiving = synra_core::link::PeerLink::new(far, peer, local, agreed, Flags::no_timeout());

    let path = std::env::temp_dir().join(format!("synra-cancelled-send-{}.bin", std::process::id()));
    std::fs::write(&path, vec![7u8; 2 * synra_core::transfer::DEFAULT_CHUNK_SIZE as usize]).expect("a file");
    let sender =
        synra_core::transfer::TransferSender::open(&path, synra_core::transfer::DEFAULT_CHUNK_SIZE).expect("a sender");
    let planned = sender.request();
    let row = super::snapshot_from(&planned, (peer, "Study PC"), "queued", 0);

    let cancel = synra_core::supervisor::Cancellation::new();
    cancel.cancel();
    let refused = super::send_ready_with(&state, peer, &mut sending, sender, row, Some(&cancel))
        .expect_err("the token was already cancelled");
    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);

    match receiving.receive_inbound().expect("the abort") {
        synra_core::link::Inbound::Envelope(envelope) => {
            assert_eq!(envelope.event.as_str(), "file.transfer.abort", "the peer was told");
        }
        other => panic!("expected an abort, got {other:?}"),
    }
    assert!(state.transfers.is_empty(), "and the row is gone");

    let _ = std::fs::remove_file(path);
}

#[test]
fn cancelling_a_transfer_removes_the_file_it_created() {
    // ★ The gap this closes: a cancelled message was removed from the panel and left on disk. The registry is the only place
    // that knows the path — a sent file belongs to the user and must survive, so "delete on cancel" cannot be a guess.
    let state = crate::testing::state();
    let path = std::env::temp_dir().join(format!("synra-cancel-{}.txt", std::process::id()));
    std::fs::write(&path, b"a message").expect("a file");
    let id = TransferId::new();
    state.transfers.record_with_temp(snapshot(id, "active"), path.clone());
    let subscriber = state.events.subscribe();

    super::transfer_cancel(&state, &id.to_string()).expect("it is in flight");

    assert!(!path.exists(), "the file went with the transfer");
    let delivery = state.events.take(subscriber).expect("announced");
    assert!(matches!(
        delivery.events(),
        [synra_ipc::event::KernelEvent::TransferFinished { .. }]
    ));
}

#[test]
fn a_second_report_keeps_the_file_the_first_one_registered() {
    // ★ `announce_transfer` runs on every block; a `record` that dropped the path would forget what to delete on the second
    // one — exactly when a cancel is likely to arrive.
    let state = crate::testing::state();
    let path = std::env::temp_dir().join(format!("synra-keep-{}.txt", std::process::id()));
    let id = TransferId::new();
    state.transfers.record_with_temp(snapshot(id, "active"), path.clone());

    super::announce_transfer(&state, snapshot(id, "active"));
    let forgotten = state.transfers.forget(&id);

    assert_eq!(forgotten.as_deref(), Some(path.as_path()), "the path survived the second report");
}

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

    let refused = super::transfer_send_text(&state, &request).expect_err("nothing to dial");

    assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    // ★ The outbox is under the app's own cache directory, not the system one: a file holding what a person typed must not sit
    // where every other process on the machine can read it.
    let outbox = state.paths.cache_dir().join("outbox");
    assert!(!outbox.exists(), "no outbox was created: {}", outbox.display());
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

// ── Incoming transfers ──────────────────────────────────────────────────────────────────────────

/// A progress step, as the listener reports it.
fn progress_step(
    transfer_id: TransferId,
    bundle: Option<&str>,
    received: u64,
    blocks_done: u32,
) -> synra_core::listener::InboundProgressEvent {
    synra_core::listener::InboundProgressEvent {
        peer: DeviceId::new(),
        progress: synra_core::session::InboundProgress {
            transfer_id,
            file_name: "photo.png".to_owned(),
            bundle: bundle.map(|id| synra_protocol::ids::PluginId::parse(id).expect("a plugin id")),
            received_bytes: received,
            total_bytes: 2_048,
            blocks_done,
            blocks_total: 4,
        },
    }
}

#[test]
fn an_incoming_step_becomes_a_receive_row() {
    // ★ `direction` is the field that makes the panel say "from" rather than "to"; a receiving row without it would be
    // drawn as a send, which is the one wrong answer a user would act on.
    let id = TransferId::new();

    let row = super::received_snapshot(&progress_step(id, None, 512, 1), "Study PC", 1_000);

    assert_eq!(row.transfer_id, id);
    assert_eq!(row.direction, "receive");
    assert_eq!(row.kind, "attachment");
    assert_eq!(row.state, "active");
    assert_eq!(row.peer_name, "Study PC");
    assert_eq!(row.file_name, "photo.png");
    assert_eq!(row.received_bytes, 512);
    assert_eq!(row.total_bytes, 2_048);
    assert_eq!(row.blocks_done, 1);
    assert_eq!(row.blocks_total, 4);
}

#[test]
fn a_bundle_in_the_making_is_named_as_one() {
    let row = super::received_snapshot(&progress_step(TransferId::new(), Some("@acme/chat"), 0, 0), "Laptop", 1_000);

    assert_eq!(row.kind, "plugin-bundle");
}

#[test]
fn finishing_an_incoming_transfer_closes_its_row_and_announces_it() {
    // ★ The receiving half of `finish_transfer`: nothing else will remove the row, because a received transfer has no
    // worker that owns it. The completed snapshot comes first so the panel can draw 100% before the row goes.
    let state = crate::testing::state();
    let id = TransferId::new();
    state.transfers.record(snapshot(id, "active"));
    let subscriber = state.events.subscribe();

    super::complete_received(&state, id);

    assert!(state.transfers.is_empty(), "the row is gone");
    let delivery = state.events.take(subscriber).expect("announced");
    let events = delivery.events();
    assert!(
        matches!(events.first(), Some(synra_ipc::event::KernelEvent::TransferUpdated(row)) if row.state == "completed"),
        "the completed row is not announced first: {events:?}"
    );
    assert!(
        matches!(events.last(), Some(synra_ipc::event::KernelEvent::TransferFinished { transfer_id }) if *transfer_id == id),
        "the panel is not told the transfer ended: {events:?}"
    );
}
