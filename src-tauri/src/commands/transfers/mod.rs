//! The transfers this shell is running, and the command that lists them.
//!
//! # Why a registry rather than a read from the link
//!
//! Because a transfer outlives the frame that started it: it is a sequence of chunks, and the interface has to be able to ask
//! "what is in flight" between two of them. The registry is written by the worker that moves the chunks and read by
//! `transfer_list`; the two never hold the lock for longer than a lookup.
//!
//! # Why the snapshot is stored rather than derived
//!
//! The worker already has the numbers — how far along, how many blocks — and deriving them again here would be a second
//! implementation of the progress arithmetic. The snapshot is what the worker last reported, and this module's job is to
//! keep that honest rather than to improve on it.

use std::collections::HashMap;

use parking_lot::Mutex;
use synra_ipc::IpcError;
use synra_ipc::dto::TransferSnapshot;
use synra_protocol::ids::TransferId;

/// Cleanup for received artifacts and shared clocks.
mod cleanup;
/// Offers and sender consent.
mod offers;
/// Pairing bookkeeping shared by dialled transfers.
mod pairing;
/// Incoming transfers and inbox files.
mod receive;
/// The in-flight transfer registry.
mod registry;
/// Shared sender orchestration.
mod send_core;
/// File-send preparation and dispatch.
mod send_file;
/// Text-send preparation and dispatch.
mod send_text;

pub use cleanup::*;
pub use offers::*;
pub use receive::*;
pub use registry::*;
pub use send_core::*;
pub use send_file::*;
pub use send_text::*;

#[cfg(test)]
use offers::remember_peer;
use pairing::record_pairing;
#[cfg(test)]
use receive::inbox_file;
use send_file::outbox_path;

#[cfg(test)]
mod tests;
