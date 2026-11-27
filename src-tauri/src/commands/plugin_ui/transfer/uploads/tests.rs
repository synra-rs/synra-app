use std::{
    io::Write as _,
    sync::{Barrier, mpsc},
    time::Duration,
};

use super::*;

fn owner(nonce: u128) -> Owner {
    Owner {
        plugin: PluginId::parse("@acme/send").unwrap(),
        nonce,
    }
}

fn begin(uploads: &Uploads, root: &Path, owner: &Owner, bytes: u64, now: u64) -> Uuid {
    uploads
        .begin(
            root,
            OpenUpload {
                owner: owner.clone(),
                to: DeviceId::new(),
                file_name: "file.bin",
                total: bytes,
            },
            now,
            || true,
        )
        .unwrap()
        .0
}

const DEADLINE: Duration = Duration::from_secs(3);

#[test]
fn two_appends_at_one_offset_commit_once() {
    let root = crate::testing::Root::fresh("concurrent-upload");
    let uploads = Uploads::default();
    let owner = owner(1);
    let id = begin(&uploads, root.path(), &owner, 2, 0);
    let barrier = Barrier::new(3);
    std::thread::scope(|scope| {
        let append = || {
            barrier.wait();
            uploads.append(&owner, id, 0, b"ok", 1)
        };
        let first = scope.spawn(append);
        let second = scope.spawn(append);
        barrier.wait();
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(result, Ok((2, 2))))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(Failure::Rule(UploadError::Offset { expected: 2 }))
                ))
                .count(),
            1
        );
    });
    let sent = TransferId::new();
    assert_eq!(
        uploads
            .finish(&owner, id, 2, |_, spool| {
                assert_eq!(std::fs::read(spool.path()).unwrap(), b"ok");
                Ok(sent)
            })
            .unwrap(),
        sent
    );
    assert_eq!(uploads.budget.reserved(), (0, 0));
}

#[test]
fn different_uploads_write_while_the_first_disk_operation_waits() {
    let root = crate::testing::Root::fresh("independent-upload");
    let uploads = Uploads::default();
    let owner = owner(1);
    let first = begin(&uploads, root.path(), &owner, 2, 0);
    let second = begin(&uploads, root.path(), &owner, 2, 0);
    let slot = uploads.lookup(&owner, first).unwrap();
    std::thread::scope(|scope| {
        let (ready, entered) = mpsc::channel();
        let (release, proceed) = mpsc::channel();
        let write = scope.spawn(move || {
            slot.pending
                .lock()
                .append(UploadPolicy::default(), 0, b"ab", 1, |file, bytes| {
                    ready.send(()).unwrap();
                    proceed.recv_timeout(DEADLINE).unwrap();
                    file.write_all(bytes)
                })
        });
        entered.recv_timeout(DEADLINE).unwrap();
        let (done, result) = mpsc::channel();
        let uploads_ref = &uploads;
        let owner_ref = &owner;
        let contender = scope.spawn(move || {
            done.send(uploads_ref.append(owner_ref, second, 0, b"cd", 1))
                .unwrap()
        });
        let independent = result.recv_timeout(DEADLINE);
        release.send(()).unwrap();
        assert!(write.join().unwrap().is_ok());
        contender.join().unwrap();
        assert!(matches!(independent, Ok(Ok((2, 2)))));
    });
}

#[test]
fn partial_write_failure_closes_the_slot_and_cleans_up() {
    let root = crate::testing::Root::fresh("write-failure");
    let uploads = Uploads::default();
    let owner = owner(1);
    let id = begin(&uploads, root.path(), &owner, 2, 0);
    let slot = uploads.lookup(&owner, id).unwrap();
    let error = slot
        .pending
        .lock()
        .append(UploadPolicy::default(), 0, b"ab", 1, |file, bytes| {
            file.write_all(&bytes[..1])?;
            Err(io::Error::other("injected partial write"))
        });
    assert!(matches!(error, Err(Failure::Io(_))));
    assert!(matches!(
        uploads.append(&owner, id, 0, b"ab", 2),
        Err(Failure::Rule(UploadError::Closed))
    ));
    assert_eq!(uploads.budget.reserved(), (0, 0));
    assert_eq!(
        std::fs::read_dir(spool::root(root.path())).unwrap().count(),
        0
    );
}

#[test]
fn quota_counts_the_worker_spool_until_worker_exit() {
    let root = crate::testing::Root::fresh("upload-handoff");
    let mut uploads = Uploads::default();
    uploads.policy.slots = 1;
    let owner = owner(1);
    let id = begin(&uploads, root.path(), &owner, 2, 0);
    uploads.append(&owner, id, 0, b"ab", 1).unwrap();
    let mut worker = None;
    uploads
        .finish(&owner, id, 2, |_, spool| {
            worker = Some(spool);
            Ok(TransferId::new())
        })
        .unwrap();
    assert!(!uploads.abort(&owner, id));
    uploads.revoke(owner.nonce);
    let worker = worker.unwrap();
    assert!(worker.path().exists());
    assert_eq!(uploads.budget.reserved(), (1, 2));
    let blocked = uploads.begin(
        root.path(),
        OpenUpload {
            owner,
            to: DeviceId::new(),
            file_name: "other.bin",
            total: 1,
        },
        3,
        || true,
    );
    assert!(matches!(blocked, Err(Failure::Rule(UploadError::Capacity))));
    drop(worker);
    assert_eq!(uploads.budget.reserved(), (0, 0));
}

#[test]
fn scope_and_expiry_do_not_leak_or_reopen_a_slot() {
    let root = crate::testing::Root::fresh("upload-owner");
    let uploads = Uploads::default();
    let original = owner(1);
    let reopened = owner(2);
    let id = begin(&uploads, root.path(), &original, 2, 0);
    assert!(!uploads.abort(&reopened, id));
    assert!(matches!(
        uploads.append(&reopened, id, 0, b"ab", 1),
        Err(Failure::Rule(UploadError::Closed))
    ));
    uploads.sweep(uploads.policy.idle_ms);
    assert!(!uploads.abort(&original, id));
    let replacement = begin(&uploads, root.path(), &reopened, 2, 0);
    uploads.revoke(original.nonce);
    assert!(uploads.lookup(&reopened, replacement).is_some());
    uploads.close();
    assert!(uploads.lookup(&reopened, replacement).is_none());
    assert_eq!(uploads.budget.reserved(), (0, 0));
}

#[test]
fn abort_racing_handoff_cannot_delete_the_workers_source() {
    let root = crate::testing::Root::fresh("abort-handoff");
    let uploads = Uploads::default();
    let owner = owner(1);
    let id = begin(&uploads, root.path(), &owner, 2, 0);
    uploads.append(&owner, id, 0, b"ab", 1).unwrap();
    std::thread::scope(|scope| {
        let (offered, worker) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        let uploads_ref = &uploads;
        let owner_ref = &owner;
        let started = scope.spawn(move || {
            uploads_ref.finish(owner_ref, id, 2, |_, spool| {
                offered.send(spool).unwrap();
                wait.recv_timeout(DEADLINE).unwrap();
                Ok(TransferId::new())
            })
        });
        let spool = worker.recv_timeout(DEADLINE).unwrap();
        let (aborting, abort_started) = mpsc::channel();
        let aborted = scope.spawn(move || {
            aborting.send(()).unwrap();
            uploads_ref.abort(owner_ref, id)
        });
        abort_started.recv_timeout(DEADLINE).unwrap();
        resume.send(()).unwrap();
        assert!(started.join().unwrap().is_ok());
        assert!(!aborted.join().unwrap());
        assert!(spool.path().is_file());
        assert!(matches!(
            uploads.append(&owner, id, 2, b"cd", 3),
            Err(Failure::Rule(UploadError::Closed))
        ));
        drop(spool);
        assert_eq!(uploads.budget.reserved(), (0, 0));
    });
}

#[test]
fn refused_worker_admission_reclaims_the_file_and_lease() {
    let root = crate::testing::Root::fresh("refused-handoff");
    let uploads = Uploads::default();
    let owner = owner(1);
    let id = begin(&uploads, root.path(), &owner, 2, 0);
    uploads.append(&owner, id, 0, b"ab", 1).unwrap();
    let error = uploads.finish(&owner, id, 2, |_, spool| {
        assert!(spool.path().is_file());
        Err(IpcError::unknown(synra_protocol::ErrorCode::Internal))
    });
    assert!(matches!(error, Err(Failure::Admission(_))));
    assert_eq!(uploads.budget.reserved(), (0, 0));
    assert_eq!(
        std::fs::read_dir(spool::root(root.path())).unwrap().count(),
        0
    );
}

#[test]
fn startup_sweep_requires_an_owned_slot_shape() {
    let cache = crate::testing::Root::fresh("startup-upload");
    let root = spool::root(cache.path());
    let abandoned = root.join(format!("upload-{}", Uuid::new_v4()));
    let unrelated = root.join("photos");
    let unexpected = root.join(format!("upload-{}", Uuid::new_v4()));
    for directory in [&abandoned, &unrelated, &unexpected] {
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("original.bin"), b"payload").unwrap();
    }
    std::fs::create_dir(unexpected.join("nested")).unwrap();
    spool::sweep_startup(cache.path());
    assert!(!abandoned.exists());
    assert!(unrelated.join("original.bin").is_file());
    assert!(unexpected.join("nested").is_dir());
}
