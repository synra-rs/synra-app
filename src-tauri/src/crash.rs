//! A file a phone can hand back, because a phone has no console.
//!
//! # Why this exists
//!
//! Android starts the application through JNI, so `eprintln!` goes nowhere a
//! person can read, and a panic that crosses that boundary aborts the process
//! with an empty log. The same is true of the compatibility layers some
//! HarmonyOS devices run Android applications under: an abort there looks like
//! the application closing by itself, and there is nothing to look at
//! afterwards.
//!
//! So two things are written to `<app data>/crash.log`:
//!
//! * every **breadcrumb** — one line per startup step, so the last line names
//!   the step the process did not come back from;
//! * every **panic** — message, location and backtrace, from any thread.
//!
//! # Why a plain file and not the diagnostics page
//!
//! The page is the thing that is not running when this is needed. A file can be
//! pulled with `adb pull` without the application cooperating, and on a desktop
//! it can simply be opened.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where this run's log goes, once the data directory is known.
static LOG: OnceLock<PathBuf> = OnceLock::new();

/// The file's name inside the data directory.
pub const FILE_NAME: &str = "crash.log";

/// Points the log at a directory and starts recording panics.
///
/// Called once, as early as the data directory is known. A second call is
/// ignored: the first directory is the one the process is running in, and a
/// hook that could be replaced is a hook a later mistake can silence.
pub fn install(data_dir: &Path) {
    let path = data_dir.join(FILE_NAME);
    if LOG.set(path.clone()).is_err() {
        return;
    }
    // A run's log is about that run. Keeping the previous one would put this
    // launch's breadcrumbs after last launch's panic and read as one story.
    let _ = std::fs::remove_file(&path);

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no message".to_owned());
        let location = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_else(|| "an unknown place".to_owned());
        let backtrace = std::backtrace::Backtrace::force_capture();
        note(&format!("PANIC at {location}: {message}\n{backtrace}"));
        previous(info);
    }));
}

/// Appends one line to the log, when there is one to append to.
///
/// Never fails and never allocates on the refusal path: a breadcrumb that could
/// stop the application would be a worse fault than the one it is reporting.
pub fn note(line: &str) {
    let Some(path) = LOG.get() else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let stamp = synra_store::sqlite::now_ms();
    let _ = writeln!(file, "{stamp} {line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_before_install_is_dropped_rather_than_panicking() {
        // The order is the caller's to get right; what must not happen is that
        // getting it wrong takes the process down before there is a log at all.
        note("before the directory is known");
    }

    #[test]
    fn a_note_after_install_lands_in_the_file() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        install(directory.path());
        note("started");
        let written = std::fs::read_to_string(directory.path().join(FILE_NAME))
            .expect("the log this test just wrote");
        assert!(written.contains("started"), "wrote: {written}");
    }
}