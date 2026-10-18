//! One running process per data directory.
//!
//! # Why a lock file rather than a Tauri plugin
//!
//! Because the second instance must be stopped **before** it binds a listener port and opens the same database, and the
//! thing that decides that is a file the two processes can contend for — not an event in a window that the second instance
//! has not created. `File::try_lock` is the operating system's own answer (an exclusive lock on Windows, `flock` on Unix),
//! so a process that is killed releases it without a cleanup path, which a pid file written by hand would not.
//!
//! # Why a second instance writes a file instead of talking to the first
//!
//! Because the message is one bit — "come to the front" — and the channel for it would otherwise be a local socket with its
//! own authentication. The running process watches `<data>/instance.show` and removes it when it has been honoured; a
//! half-second poll is cheap for the one process that exists, and it has no failure mode where a connection is refused
//! because the window is busy.
//!
//! # What this is not
//!
//! Not a security boundary. A second process could ignore the lock and start anyway; the lock is what a well-behaved second
//! launch obeys, and the misbehaving one is the same user's problem as running the binary twice.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// The lock file, inside the data directory.
pub const LOCK_FILE: &str = "instance.lock";
/// The file a second instance writes to ask the first to come forward.
pub const SHOW_REQUEST: &str = "instance.show";

/// The lock while this process runs.
///
/// Dropping it releases the operating system's lock, which is why it is managed by the app rather than stored in a static
/// that nobody would ever drop.
#[derive(Debug)]
pub struct Guard {
    /// Held only for the lock; never read or written after the claim.
    _file: File,
    /// The path a second instance writes when it wants the window.
    request: PathBuf,
}

impl Guard {
    /// Where a second instance's "show" request will appear.
    #[must_use]
    pub fn request_path(&self) -> &Path {
        &self.request
    }
}

/// What a launch found.
#[derive(Debug)]
pub enum Start {
    /// This process owns the data directory.
    First(Guard),
    /// Another process already owns it, and has been asked to come forward.
    Second,
}

/// Claims the data directory, or reports that someone else has it.
///
/// # Errors
///
/// The operating-system error when the lock file cannot be opened or locked for a reason other than contention — a
/// read-only directory, say. A caller may choose to run anyway: two instances is worse than one, but an application that
/// will not open is worse than both.
pub fn claim(data_dir: &Path) -> io::Result<Start> {
    std::fs::create_dir_all(data_dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data_dir.join(LOCK_FILE))?;

    match file.try_lock() {
        Ok(()) => {
            let request = data_dir.join(SHOW_REQUEST);
            // ★ A request left by a **previous** run is not honoured now: it was written for a process that is gone, and
            // replaying it would pop the window on every start after the one that was asked to.
            let _ = std::fs::remove_file(&request);
            Ok(Start::First(Guard { _file: file, request }))
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            std::fs::write(data_dir.join(SHOW_REQUEST), std::process::id().to_string())?;
            Ok(Start::Second)
        }
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

/// Watches for a second instance's request, and brings the window forward when one arrives.
///
/// The thread lives as long as the process: there is nothing to cancel it towards, because the thing it exists for is the
/// process's lifetime, and an exit ends it. It is started **after** the window exists — a request honoured before there is
/// a window would find nothing to show.
#[cfg(feature = "tauri-shell")]
pub fn watch(app: tauri::AppHandle, request: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("synra-instance".to_owned())
        .spawn(move || loop {
            if request.exists() {
                // Removed before the show, so a request that arrives while the window is coming forward is a new one
                // rather than the same one seen twice.
                let _ = std::fs::remove_file(&request);
                crate::window::show_main(&app);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        });
    if let Err(error) = spawned {
        eprintln!("synra: the single-instance watcher could not start: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::{SHOW_REQUEST, Start, claim};

    fn scratch() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("synra-instance-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn a_second_launch_is_told_the_first_is_running_and_asks_it_forward() {
        let directory = scratch();
        let first = claim(&directory).expect("the directory is writable");
        assert!(matches!(first, Start::First(_)), "the first launch takes the lock");

        let second = claim(&directory).expect("contention is an answer, not an error");
        assert!(matches!(second, Start::Second), "the second launch must not proceed");
        assert!(
            directory.join(SHOW_REQUEST).exists(),
            "the second launch did not ask the first to come forward"
        );

        drop(first);
        let again = claim(&directory).expect("the lock was released with the guard");
        assert!(matches!(again, Start::First(_)), "a restart can take the lock");
        assert!(
            !directory.join(SHOW_REQUEST).exists(),
            "a request left by the previous run is replayed on start"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn two_data_directories_do_not_contend() {
        // Two installations (or a test and a real one) must be able to run side by side; the lock is per directory, not a
        // global mutex.
        let one = scratch();
        let two = scratch();

        assert!(matches!(claim(&one).expect("writable"), Start::First(_)));
        assert!(matches!(claim(&two).expect("writable"), Start::First(_)));

        let _ = std::fs::remove_dir_all(&one);
        let _ = std::fs::remove_dir_all(&two);
    }
}
