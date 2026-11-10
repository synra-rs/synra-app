//! Diagnostics: what a user sends when something is wrong.

use synra_ipc::{DiagExportOptions, DiagExportResult, IpcError, ProbeReportDto, ProbeResultDto};
use synra_observe::diagnostics::{
    BundlePlan, BundleSection, BundleSources, DeviceRecord, EnvSummary, PluginRecord,
};
use synra_observe::doctor::{HostFacts, ProbeReport, ProbeTargets, run_probes};
use synra_protocol::ErrorCode;

use crate::state::AppState;

mod export;
mod probes;
mod runtime;

pub use export::diag_export;
pub use probes::diag_probes;
pub(crate) use runtime::{configuration, destination, environment, internal};
pub use runtime::{diag_crash_dismiss, diag_crashes, diag_logs_tail, diag_metrics};

#[cfg(test)]
mod tests;
