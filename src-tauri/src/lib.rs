//! The Tauri shell's logic, with no window in it.
//!
//! # What this crate is
//!
//! The design document (`08-ipc-and-frontend.md` §2.2) lists thirty-eight commands. Each one needs three things: a name
//! the interface can call, a request the interface can send, and an answer the interface can render. Only the middle one
//! is a business rule. The other two are Tauri plumbing.
//!
//! So this is the middle one, and `main.rs` is the other two. Nothing here imports Tauri, which means every command in
//! this crate is unit-testable on a machine with no WebView2, no NDK and no window — and a test that cannot run on a
//! developer's machine is a test that does not exist.
//!
//! # Why the state is a trait and not a concrete kernel
//!
//! Because a command that talks to a concrete kernel can only be tested against a running kernel, which needs a database,
//! a socket and a data directory. A command that talks to a trait is tested against a fake that answers in a table, and
//! then the tests are about *the command*: does it check its input, does it map the kernel's answer onto the DTO the
//! interface expects, does it refuse rather than guess. Those are the parts that are wrong in the ways users see.
//!
//! The price is real and stated here: a trait with thirty-eight methods is a wide interface, and a wide interface is
//! harder to implement than a narrow one. It is the right trade because the alternative — testing commands only through
//! a live kernel — has already been tried in this project's history and it is what left the shell unbuilt.
//!
//! # What is not here
//!
//! No window, no menu, no tray icon, no window-state persistence. Those are `main.rs`'s, and the design document keeps
//! them there deliberately: they are the parts that need a platform and cannot be unit-tested, so keeping them out of
//! this crate keeps them out of the way of the parts that can.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// ★ Every command returns `Result<T, IpcError>`, and `IpcError` carries a translated message, its parameters and its
// suggested actions — so it is a large error type by design, and boxing it would put an allocation on every refusal path
// to save a lint that fires because the DTO is honest about what the interface needs. The kernel sets the same precedent.
#![allow(clippy::result_large_err)]

// Test code is exempt from the no-panic rules; see `synra-protocol` in the kernel for the reasoning.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::needless_pass_by_value,
    )
)]

pub mod commands;
pub mod plugin_host;
pub mod state;

#[cfg(test)]
mod testing;

pub use state::AppState;

/// Proves [`AppState`] can be handed to Tauri.
///
/// ★ A test rather than a comment, because the requirement is invisible until it is violated.
///
/// Tauri refuses to register a state that is not `Send + Sync`, and `rusqlite::Connection` is `Send` but not `Sync` —
/// so the natural way to hold the database, `Arc<SynraDb>`, produces a shell that fails to start with an error about
/// thread safety that points nowhere near the field. This assertion cannot be written down; it can only be made to
/// stop compiling.
#[allow(dead_code)]
const fn app_state_must_be_send_and_sync()
where
    AppState: Send + Sync,
{
}

/// The version this shell reports as the kernel's.
///
/// ★ Reported from the shell's own crate rather than from a kernel crate's version.
///
/// A transitive path dependency's version is a version the user cannot match against a bug report, and a diagnostic
/// report that names one is a diagnostic report that generates a second round of questions.
pub const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");
