//! The desktop process, and the exit code.
//!
//! # Why almost nothing is here
//!
//! Because the window itself is not a desktop-only thing. Android starts the same window the same way — through
//! [`synra_app::shell`] — and it reaches it over JNI instead of over `main`, so the assembly had to move into the
//! library (`src/shell.rs`). What is left here is the one thing a desktop process has that an activity does not: an
//! exit code to return to whoever launched it.
//!
//! Every command's body lives in the library for a different reason — so `cargo test --no-default-features` can run
//! them with no WebView in the build graph — and the assembly lives there now too. This file is the amount of code that
//! can only be checked by launching a process and reading `$LASTEXITCODE`.

fn main() -> std::process::ExitCode {
    // ★ The refusal is printed, not panicked.
    //
    // A shell that cannot open its database has nothing to show, and a panic in a windowed app on Windows is a dialog
    // that says "synra-app.exe has stopped working" — which tells a user nothing and tells whoever triages it less.
    // The message here names the actual cause.
    match synra_app::shell::run_window() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("synra: {reason}");
            std::process::ExitCode::FAILURE
        }
    }
}
