use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

mod finished;
mod kernel;
mod offer;
mod progress;

pub(super) use finished::finished_callback;
pub(super) use kernel::kernel_callback;
pub(super) use offer::offer_callback;
pub(super) use progress::progress_callback;

pub(super) type Started = Arc<Mutex<HashMap<synra_protocol::ids::TransferId, Instant>>>;

pub(super) fn new_started() -> Started {
    Arc::new(Mutex::new(HashMap::new()))
}
