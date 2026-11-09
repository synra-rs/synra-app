//! What a command has to reach to do its work.
//!
//! # Why this is concrete and not a trait
//!
//! Because a trait here would have no production implementor. The kernel already runs with no window — `synra-daemon`
//! is the proof, and it is the reason the kernel's boundary is trustworthy — so the shell does not need an abstraction
//! to make itself testable. It needs the same pieces the daemon holds, and a test builds them over an in-memory
//! database.
//!
//! Writing a thirty-eight-method trait anyway would be the shape that feels safe and is not: it would be implemented
//! once by production and once by every test, and the two would drift until the tests described a kernel that does not
//! exist.
//!
//! # Why the pieces sit behind separate locks
//!
//! Because they block for different reasons, and often enough that sharing one lock would be a defect rather than a
//! simplification. A Wasm call is bounded by fuel and a clock and can still take its whole budget; a socket read waits
//! for a peer; a database call is milliseconds. A `devices_snapshot` that waited behind a compiling plugin would be the
//! kind of bug nobody finds until a user with nine installed plugins says the app is slow.

mod errors;
mod lifecycle;
mod metrics;
mod observability;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use errors::internal;
pub use metrics::observe_db_writes;
pub(crate) use types::ScanTask;
pub use types::{AppState, HostedUi, ProcessSample};
