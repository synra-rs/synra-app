pub(crate) const MAX_PLUGIN_TRANSFER_BYTES: usize = 4 * 1024 * 1024;

/// The largest file a page may send as a **chunked** upload.
///
/// The single-call cap is a property of one IPC message, not of a transfer: a chunked upload is how a page sends a file
/// larger than that, and 64 MiB is the design's whole-package ceiling (16 §2.1).
pub(crate) const MAX_PLUGIN_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// The largest chunk one `transfer.chunk` may carry.
///
/// One mebibyte of bytes is about 1.4 MiB of base64 in the RPC payload: large enough that a 64 MiB file is 64 round trips
/// rather than hundreds, small enough that a chunk is not a whole message-budget.
pub(crate) const MAX_PLUGIN_CHUNK_BYTES: usize = 1024 * 1024;

/// A chunked upload a page is writing.
#[derive(Debug)]
pub struct Upload {
    /// Which plugin started it; only that plugin may append to or finish it.
    pub(crate) plugin_id: String,
    /// The directory this upload owns, removed with it.
    pub(crate) directory: std::path::PathBuf,
    /// The file being written.
    pub(crate) path: std::path::PathBuf,
    /// Who it is for.
    pub(crate) to: synra_protocol::ids::DeviceId,
    /// How many bytes the page declared.
    pub(crate) total: u64,
    /// How many have been written so far.
    pub(crate) written: u64,
}

/// The chunked uploads in progress, by the identifier `transfer.begin` answered.
#[derive(Debug, Default)]
pub struct Uploads {
    pub(crate) by_id: parking_lot::Mutex<std::collections::HashMap<String, Upload>>,
}

impl Uploads {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&self, id: String, upload: Upload) {
        self.by_id.lock().insert(id, upload);
    }

    /// Removes an upload, but only for the plugin that started it.
    pub(crate) fn take(&self, plugin_id: &str, id: &str) -> Option<Upload> {
        let mut uploads = self.by_id.lock();
        if uploads
            .get(id)
            .is_some_and(|upload| upload.plugin_id == plugin_id)
        {
            uploads.remove(id)
        } else {
            None
        }
    }
}

/// Removes the file and directory a chunked upload owned.
pub(crate) fn discard_upload(upload: &Upload) {
    let _ = std::fs::remove_file(&upload.path);
    let _ = std::fs::remove_dir(&upload.directory);
}
