//! Fixtures for the command tests.
//!
//! # Why this is `#[cfg(test)]` rather than behind a feature
//!
//! Because a fixture that ships is a fixture somebody will use from a binary crate, and a command test that can only be
//! run from the library is a command test that gets skipped. There is no use case for building a test fixture into a
//! release, so it is not one.

use std::path::PathBuf;

use synra_store::{Paths, SynraDb};

use crate::state::AppState;

/// A directory that removes itself.
///
/// # Why the test owns it rather than leaking one
///
/// Leaking is the usual shortcut and it costs more than it looks: a suite that leaves a directory per case behind fills
/// a developer's temp directory over a week, and the next failure is then someone's disk rather than their code.
pub struct Root {
    path: PathBuf,
}

impl Drop for Root {
    fn drop(&mut self) {
        // A failure here is a failure to clean up, not a failure of the code under test, so it is deliberately not
        // reported: there is no caller left to report it to and no assertion it could satisfy.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl Root {
    /// A directory named after what is being tested.
    ///
    /// # Panics
    ///
    /// In tests only, when the directory cannot be made — which is worth failing on, since the test that asked for it
    /// was about to depend on it.
    #[must_use]
    pub fn fresh(label: &str) -> Self {
        root(label)
    }
}

/// A state over an in-memory database, with no plugin host.
///
/// The fast path: no files, no cleanup, no filesystem permissions. Most command tests only care that a command maps an
/// answer onto a DTO, and this is the shortest path to that.
///
/// # Panics
///
/// In tests only, and only if the in-memory database cannot be opened — which would be a bug in `synra-store` that
/// every test in the suite would rather report than swallow.
pub fn state() -> AppState {
    let mut db = SynraDb::open_in_memory().expect("an in-memory database");
    db.migrate().expect("migrates");
    let paths = Paths::under(scratch());
    AppState::new(db, paths, TEST_PLATFORM, FIXED_NOW_MS).expect("an app state")
}

/// A state over a database on disk, together with the directory it lives in.
///
/// Needed by the tests that prove something survives a restart — and by only those, because everything else is faster
/// in memory and does not need the root held alive.
pub fn file_state() -> (Root, AppState) {
    let root = root("file-state");
    let state = state_in(&root);
    (root, state)
}

/// A second state over the same directory, as a restart would build.
///
/// # Panics
///
/// In tests only, when the database cannot be reopened — which means the first state wrote something the second
/// cannot read, and that is worth failing on rather than working around.
pub fn reopen(state: &AppState) -> AppState {
    let mut db = SynraDb::open(&state.paths.db_path()).expect("the database reopens");
    // Migrated, because a shell that started first does not migrate on the way out and the database on disk may be one
    // this build's schema predates. A reopen that skipped it would fail with "no such table" and look like data loss.
    db.migrate().expect("migrates");
    AppState::new(db, state.paths.clone(), TEST_PLATFORM, FIXED_NOW_MS + 1).expect("an app state")
}

/// A state over a directory the caller owns.
pub fn state_in(root: &Root) -> AppState {
    // ★ The path comes from `Paths`, not from a name written here.
    //
    // `Paths` owns where the database lives (`kernel.db` under the data directory); a fixture that wrote
    // `synra.db` beside it would open a different file, and every "survives a restart" test would then reopen an empty
    // one and fail for a reason that has nothing to do with what it was checking.
    let paths = Paths::under(root.path.clone());
    let mut db = SynraDb::open(&paths.db_path()).expect("a database");
    db.migrate().expect("migrates");
    AppState::new(db, paths, TEST_PLATFORM, FIXED_NOW_MS).expect("an app state")
}

/// A fresh directory named after the test that asked for it.
///
/// Named rather than random so that a directory found on disk after a crashed run says which test it belongs to — which
/// is the difference between a five-second answer and a guessing game.
fn root(label: &str) -> Root {    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let path = scratch().join(format!("synra-app-{label}-{unique}"));
    std::fs::create_dir_all(&path).expect("a scratch directory");
    Root { path }
}

/// The root every scratch directory sits under.
fn scratch() -> PathBuf {
    std::env::temp_dir().join("synra-app-tests")
}

/// The platform the fixtures report.
///
/// A literal rather than the running operating system, because a test that passes on Windows and fails on a colleague's
/// Mac is a test whose result describes the machine rather than the code.
pub const TEST_PLATFORM: &str = "test";

/// A clock the tests fix.
///
/// Chosen far enough from zero that a date formatter cannot mistake it for the epoch, and the reason every snapshot in
/// these tests has a timestamp someone can read.
pub const FIXED_NOW_MS: i64 = 1_767_225_600_000;
