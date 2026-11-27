use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use synra_core::transfer::upload::{UploadError, UploadPolicy, UploadUsage};
use uuid::Uuid;

use super::{Owner, spool::Paths};

#[derive(Debug, Default)]
pub(super) struct Budget(Arc<Mutex<HashMap<Uuid, Holding>>>);

#[derive(Debug)]
struct Holding {
    owner: Owner,
    bytes: u64,
    retry: Option<Paths>,
}

impl Budget {
    pub(super) fn reserve(
        &self,
        policy: UploadPolicy,
        owner: Owner,
        bytes: u64,
    ) -> Result<Lease, UploadError> {
        let mut holdings = self.0.lock();
        let mut usage = UploadUsage {
            slots: holdings.len(),
            ..UploadUsage::default()
        };
        for holding in holdings.values() {
            usage.bytes += holding.bytes;
            if holding.owner.plugin == owner.plugin {
                usage.plugin += 1;
            }
            if holding.owner == owner {
                usage.session += 1;
            }
        }
        policy.admit(usage, bytes)?;
        let id = Uuid::new_v4();
        holdings.insert(
            id,
            Holding {
                owner,
                bytes,
                retry: None,
            },
        );
        Ok(Lease {
            id,
            holdings: Some(Arc::clone(&self.0)),
        })
    }

    pub(super) fn retry_cleanup(&self) {
        let retries: Vec<_> = self
            .0
            .lock()
            .iter()
            .filter_map(|(id, held)| held.retry.clone().map(|paths| (*id, paths)))
            .collect();
        for (id, paths) in retries {
            if paths.cleanup().is_ok() {
                self.0.lock().remove(&id);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn reserved(&self) -> (usize, u64) {
        let holdings = self.0.lock();
        (
            holdings.len(),
            holdings.values().map(|entry| entry.bytes).sum(),
        )
    }
}

#[derive(Debug)]
pub(super) struct Lease {
    pub(super) id: Uuid,
    holdings: Option<Arc<Mutex<HashMap<Uuid, Holding>>>>,
}

impl Lease {
    pub(super) fn defer_cleanup(&mut self, paths: Paths) {
        if let Some(holdings) = self.holdings.take() {
            if let Some(holding) = holdings.lock().get_mut(&self.id) {
                holding.retry = Some(paths);
            }
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(holdings) = &self.holdings {
            holdings.lock().remove(&self.id);
        }
    }
}
