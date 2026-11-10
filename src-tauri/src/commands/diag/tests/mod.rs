//! Tests for the diagnostics command.
//!
//! # What these tests are really about
//!
//! A bundle is the one artefact a user hands to a stranger. Every test here is about the difference between what the
//! user was told and what is in the file — because that difference is the whole risk, and it is invisible once the file
//! has been sent.

use serde_json::Value;

use super::{DiagExportResult, diag_export, diag_metrics, diag_probes};
use crate::testing;

fn options() -> synra_ipc::DiagExportOptions {
    synra_ipc::DiagExportOptions::default()
}

/// The bundle that was written, parsed back.
fn bundle(result: &DiagExportResult) -> Value {
    let text = std::fs::read_to_string(&result.path).expect("the bundle was written");
    serde_json::from_str(&text).expect("and it is JSON")
}

/// Whether a key in a written bundle holds nothing.
///
/// A bundle's optional sections serialise as an explicit `null` rather than being absent, so "not included" has to
/// accept both spellings — and a test that only accepts one of them would pass for the wrong reason.
fn is_empty_section(file: &Value, key: &str) -> bool {
    matches!(file.get(key), None | Some(Value::Null))
}

mod bundle_sections;
mod export;
mod logs;
mod runtime;
