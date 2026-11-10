use super::*;

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
    let agreed = Agreed {
        common: Vec::new(),
        missing_required: Vec::new(),
    };
    let local = DeviceId::new();
    let peer = DeviceId::new();
    let mut sending =
        synra_core::link::PeerLink::new(near, local, peer, agreed.clone(), Flags::no_timeout());
    let mut receiving =
        synra_core::link::PeerLink::new(far, peer, local, agreed, Flags::no_timeout());

    let path = std::env::temp_dir().join(format!("synra-ready-{}.bin", std::process::id()));
    std::fs::write(
        &path,
        vec![3u8; 2 * synra_core::transfer::DEFAULT_CHUNK_SIZE as usize],
    )
    .expect("a file");
    let sender =
        synra_core::transfer::TransferSender::open(&path, synra_core::transfer::DEFAULT_CHUNK_SIZE)
            .expect("a sender");
    let planned = sender.request();
    let total =
        u32::try_from(planned.byte_length.div_ceil(u64::from(planned.chunk_size))).expect("blocks");
    let row = super::super::snapshot_from(&planned, (peer, "Study PC"), "queued", 0);
    let subscriber = state.events.subscribe();

    let destination = path.with_extension("received");
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let proposal = receiving.receive().expect("a proposal before blocks");
            let trace = proposal.trace_id;
            let mut receiver =
                synra_core::Receiving::accept(&proposal, &destination, trace).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut blocks = 0;
            loop {
                match receiver.absorb(&mut receiving, 0, deadline).unwrap() {
                    synra_core::Received::Block { index, .. } => {
                        assert_eq!(index, blocks);
                        blocks += 1;
                        synra_core::report_progress(&mut receiving, receiver.receiver(), trace, 0)
                            .unwrap();
                    }
                    synra_core::Received::Verified { .. } => {
                        assert_eq!(blocks, total);
                        break;
                    }
                    other => panic!("unexpected transfer result {other:?}"),
                }
            }
        });
        super::super::send_ready(&state, peer, &mut sending, sender, row).expect("the peer confirms");
        worker.join().unwrap();
    });
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        std::fs::read(&path).unwrap()
    );
    std::fs::remove_file(destination).unwrap();
    let delivery = state.events.take(subscriber).expect("announced");
    assert!(
        delivery.events().iter().any(|event| matches!(
            event,
            synra_ipc::event::KernelEvent::TransferFinished { .. }
        )),
        "the end was announced"
    );
    assert!(state.transfers.is_empty(), "and the row is gone");

    // ★ The attempt is measured where every start converges, so the panel's number and the diagnostic bundle's answer the
    // same question. The size is the file's, and only a delivered transfer contributes one.
    let snapshot = state.metrics.snapshot();
    assert_eq!(
        snapshot.get("transfers_total{kind=attachment,outcome=delivered}"),
        Some(&synra_observe::metrics::Value::Counter(1)),
        "the delivered attempt is counted"
    );
    match snapshot.get("transfer_bytes{kind=attachment}") {
        Some(synra_observe::metrics::Value::Histogram(histogram)) => {
            assert_eq!(histogram.count, 1);
            assert_eq!(
                histogram.sum,
                2.0 * f64::from(synra_core::transfer::DEFAULT_CHUNK_SIZE)
            );
        }
        other => panic!("expected a size histogram, got {other:?}"),
    }
    assert!(
        snapshot
            .get("transfer_duration_ms{kind=attachment}")
            .is_some(),
        "the attempt's time is recorded"
    );
    match snapshot.get("transfer_throughput_bps{kind=attachment}") {
        // The exact rate depends on how long the memory link took, so the claim is that a rate was measured at all.
        Some(synra_observe::metrics::Value::Histogram(histogram)) => {
            assert_eq!(histogram.count, 1);
            assert!(
                histogram.sum > 0.0,
                "a delivered transfer has a positive rate"
            );
        }
        other => panic!("expected a throughput histogram, got {other:?}"),
    }

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
    let agreed = Agreed {
        common: Vec::new(),
        missing_required: Vec::new(),
    };
    let local = DeviceId::new();
    let peer = DeviceId::new();
    let mut sending =
        synra_core::link::PeerLink::new(near, local, peer, agreed.clone(), Flags::no_timeout());
    let mut receiving =
        synra_core::link::PeerLink::new(far, peer, local, agreed, Flags::no_timeout());

    let path =
        std::env::temp_dir().join(format!("synra-cancelled-send-{}.bin", std::process::id()));
    std::fs::write(
        &path,
        vec![7u8; 2 * synra_core::transfer::DEFAULT_CHUNK_SIZE as usize],
    )
    .expect("a file");
    let sender =
        synra_core::transfer::TransferSender::open(&path, synra_core::transfer::DEFAULT_CHUNK_SIZE)
            .expect("a sender");
    let planned = sender.request();
    let row = super::super::snapshot_from(&planned, (peer, "Study PC"), "queued", 0);

    let cancel = synra_core::supervisor::Cancellation::new();
    cancel.cancel();
    let refused = super::super::send_ready_with(&state, peer, &mut sending, sender, row, Some(&cancel))
        .expect_err("the token was already cancelled");
    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);

    match receiving.receive_inbound().expect("the abort") {
        synra_core::link::Inbound::Envelope(envelope) => {
            assert_eq!(
                envelope.event.as_str(),
                "file.transfer.abort",
                "the peer was told"
            );
        }
        other => panic!("expected an abort, got {other:?}"),
    }
    assert!(state.transfers.is_empty(), "and the row is gone");

    // ★ A cancelled attempt is counted as one, and it contributes **no size**: the histogram answers "how large are the
    // files this device sends", and a file that never left has no answer there. The time is still recorded, because the
    // attempt really happened.
    let snapshot = state.metrics.snapshot();
    assert_eq!(
        snapshot.get("transfers_total{kind=attachment,outcome=cancelled}"),
        Some(&synra_observe::metrics::Value::Counter(1)),
        "the cancelled attempt is counted"
    );
    assert_eq!(
        snapshot.get("transfer_bytes{kind=attachment}"),
        None,
        "no bytes were delivered, so no size is recorded"
    );

    let _ = std::fs::remove_file(path);
}
