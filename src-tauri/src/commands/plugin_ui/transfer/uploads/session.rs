use std::{fs::File, io};

use synra_core::transfer::upload::{UploadEnd, UploadError, UploadPolicy, UploadProgress};
use synra_ipc::IpcError;
use synra_protocol::TransferId;

use super::{Failure, UploadSpool};

#[derive(Debug)]
pub(super) struct Pending {
    state: UploadProgress,
    // Close the write handle before removing the file, including when the slot is dropped.
    file: Option<File>,
    spool: Option<UploadSpool>,
}

impl Pending {
    pub(super) fn new(total: u64, now_ms: u64, file: File, spool: UploadSpool) -> Self {
        Self {
            state: UploadProgress::new(total, now_ms),
            file: Some(file),
            spool: Some(spool),
        }
    }

    pub(super) fn append(
        &mut self,
        policy: UploadPolicy,
        offset: u64,
        bytes: &[u8],
        now_ms: u64,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<(u64, u64), Failure> {
        if self.expire(policy, now_ms) {
            return Err(UploadError::Closed.into());
        }
        let end = self.state.append_end(policy, offset, bytes.len())?;
        let file = self.file.as_mut().ok_or(UploadError::Closed)?;
        if let Err(error) = write(file, bytes) {
            self.close(UploadEnd::Failed);
            return Err(error.into());
        }
        self.state.commit_append(end, now_ms);
        Ok((end, self.state.total()))
    }

    pub(super) fn finish(
        &mut self,
        policy: UploadPolicy,
        now_ms: u64,
        launch: impl FnOnce(UploadSpool) -> Result<TransferId, IpcError>,
    ) -> Result<TransferId, Failure> {
        if self.expire(policy, now_ms) {
            return Err(UploadError::Closed.into());
        }
        let file = self.file.as_ref().ok_or(UploadError::Closed)?;
        let measured = file.metadata();
        let ready = measured
            .map_err(Failure::from)
            .and_then(|meta| self.state.begin_handoff(meta.len()).map_err(Failure::from));
        if let Err(error) = ready {
            self.close(UploadEnd::Failed);
            return Err(error);
        }
        drop(self.file.take());
        let spool = self.spool.take().ok_or(UploadError::Closed)?;
        match launch(spool) {
            Ok(id) => {
                self.state.transferred();
                Ok(id)
            }
            Err(error) => {
                self.state.close(UploadEnd::Failed);
                Err(Failure::Admission(error))
            }
        }
    }

    pub(super) fn close(&mut self, end: UploadEnd) -> bool {
        if !self.state.close(end) {
            return false;
        }
        drop(self.file.take());
        drop(self.spool.take());
        true
    }

    pub(super) fn expire(&mut self, policy: UploadPolicy, now_ms: u64) -> bool {
        self.state.expired(policy, now_ms) && self.close(UploadEnd::Expired)
    }

    pub(super) fn ended(&self) -> bool {
        self.file.is_none()
    }
}
