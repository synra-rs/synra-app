//! Real command, SQLite, restart and cross-command consent tests.

use std::sync::{Barrier, mpsc};
use std::thread;
use std::time::Duration;

use synra_ipc::event::KernelEvent;
use synra_protocol::{DeviceId, ErrorCode, TransferId};

use crate::commands::{config, devices, transfers};
use crate::state::AppState;
use crate::testing;
use crate::transfer_desk::Pending;

pub(super) fn pair(state: &AppState, peer: &str) {
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: peer.to_owned(),
            display_name: "Phone".to_owned(),
            platform: "test".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1,
            address: None,
        })
        .unwrap();
}

fn pending(id: &str, peer: &str) -> Pending {
    Pending {
        transfer_id: id.to_owned(),
        peer: peer.to_owned(),
        file_name: "notes.txt".to_owned(),
        total_bytes: 3,
        started_at_ms: 1,
    }
}

fn with_offer(state: &AppState, peer: &str, action: impl FnOnce(&str)) -> bool {
    let id = TransferId::new().to_string();
    let question = pending(&id, peer);
    thread::scope(|scope| {
        let (ready, announced) = mpsc::channel();
        let asked = scope.spawn(move || {
            state.transfer_desk.ask(
                &question,
                || ready.send(()).unwrap(),
                Duration::from_secs(5),
            )
        });
        announced.recv_timeout(Duration::from_secs(2)).unwrap();
        action(&id);
        asked.join().unwrap().expect("offer admitted")
    })
}

mod preferences;
mod queue;
mod revocation;
