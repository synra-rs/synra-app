//! Generates what Tauri needs before the crate compiles.
//!
//! # Why this file exists at all
//!
//! `tauri_build::build()` reads `tauri.conf.json` at **compile time** and writes the generated context the crate then
//! links against. Without it the shell compiles into a binary that has no window and no `AppHandle` wiring, and the
//! failure shows up at the first `tauri::generate_handler!` — which names neither this file nor the missing step.
//!
//! # Why it is gated on the feature
//!
//! ★ Because `tauri-build` **cannot run** without `tauri` in the graph, and that is not documented anywhere obvious.
//!
//! `tauri_build::is_dev()` reads `DEP_TAURI_DEV`, which is emitted by the `tauri` crate's own build script — `tauri`
//! declares `links = "Tauri"`. Call `tauri_build::build()` from a `--no-default-features` build and it panics with
//! `missing \`cargo:dev\` instruction, please update tauri to latest`, which reads as a version mismatch and is not:
//! `tauri` is already current, it is simply not a dependency of that build.
//!
//! So the gate is not an optimisation. Without it the library's own tests — every command body, with no WebView in the
//! build graph — cannot be compiled at all, which is the one thing this crate's structure exists to make possible.
//!
//! A build script is compiled with the package's active features, so `#[cfg(feature = …)]` here is the package's own
//! `tauri-shell`, and the two stay in step without either naming the other.

fn main() {
    #[cfg(feature = "tauri-shell")]
    permissions::build();
}

#[cfg(feature = "tauri-shell")]
#[path = "build/permissions.rs"]
mod permissions;
