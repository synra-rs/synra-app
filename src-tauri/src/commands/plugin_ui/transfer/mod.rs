mod direct;
mod upload_rpc;
mod uploads;

pub(crate) const TRANSFER_GRANT: &str = "file:transfer:send";

#[cfg(any(feature = "tauri-shell", test))]
pub(crate) use direct::owned_status;
pub(crate) use direct::transfer_status;
#[cfg(feature = "tauri-shell")]
pub(crate) use upload_rpc::upload_time_ms;
pub(crate) use upload_rpc::{transfer_abort, transfer_begin, transfer_chunk, transfer_finish};
pub(crate) use uploads::UploadSpool;
pub use uploads::Uploads;
#[cfg(feature = "tauri-shell")]
pub(crate) use uploads::sweep_startup as sweep_upload_spool;
