use super::*;

#[test]
fn a_second_report_keeps_the_file_the_first_one_registered() {
    // ★ `announce_transfer` runs on every block; a `record` that dropped the path would forget what to delete on the second
    // one — exactly when a cancel is likely to arrive.
    let state = crate::testing::state();
    let path = std::env::temp_dir().join(format!("synra-keep-{}.txt", std::process::id()));
    let id = TransferId::new();
    state
        .transfers
        .record_with_temp(snapshot(id, "active"), path.clone());

    super::super::announce_transfer(&state, snapshot(id, "active"));
    let forgotten = state.transfers.forget(&id);

    assert_eq!(
        forgotten.as_deref(),
        Some(path.as_path()),
        "the path survived the second report"
    );
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
        digest: synra_protocol::session::SriDigest::new(
            synra_protocol::session::DigestAlgorithm::Sha256,
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU",
        )
        .expect("a digest"),
        sync_session_id: None,
        meta: TransferMeta::PluginBundle {
            plugin_id: PluginId::parse("@acme/hello").expect("a plugin id"),
            version: "1.2.3".to_owned(),
        },
    };
    let peer = (DeviceId::new(), "Study PC");

    let snapshot = super::super::snapshot_from(&request, peer, "active", 1_000);

    assert_eq!(
        snapshot.peer_name, "Study PC",
        "the name is the link's, not the request's"
    );
    assert_eq!(snapshot.kind, "plugin-bundle");
    assert_eq!(
        snapshot.file_name, "@acme/hello 1.2.3",
        "what the row names"
    );
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
        digest: synra_protocol::session::SriDigest::new(
            synra_protocol::session::DigestAlgorithm::Sha256,
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU",
        )
        .expect("a digest"),
        sync_session_id: None,
        meta: TransferMeta::Attachment {
            file_name: "notes.txt".to_owned(),
            mime_type: None,
            context_id: None,
        },
    };

    let snapshot = super::super::snapshot_from(&request, (DeviceId::new(), "Laptop"), "queued", 0);

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

    super::super::announce_transfer(&state, snapshot(id, "active"));

    assert_eq!(super::super::transfer_list(&state).len(), 1, "the registry has it");
    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::TransferUpdated(transfer)] => {
            assert_eq!(transfer.transfer_id, id)
        }
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

    let ordered: Vec<TransferId> = transfers
        .list()
        .into_iter()
        .map(|t| t.transfer_id)
        .collect();

    let (first, second) = if one < two { (one, two) } else { (two, one) };
    assert_eq!(ordered, vec![first, second], "sorted by identifier");
}

// ── Incoming transfers ──────────────────────────────────────────────────────────────────────────
