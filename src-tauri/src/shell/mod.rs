//! The window: what it is built over, and what the interface may call.
//!
//! # Why this module is in the library and not in `main.rs`
//!
//! Because Android has no binary entry point to put it in. The generated `gen/android` project is a Kotlin activity
//! that loads a **shared library** and calls one C symbol, `start_app`; that symbol is produced by
//! `#[tauri::mobile_entry_point]` applied to a function in the library target. A `main.rs` is never linked into that
//! library, so `main.rs` is the one file on Android that cannot hold anything.
//!
//! The desktop binary keeps its own `main.rs`, and it keeps it small: it calls [`run_window`] here and turns the answer
//! into an exit code. So the two platforms share one assembly of the window and neither duplicates the other's.
//!
//! # Why it is behind the `tauri-shell` feature
//!
//! Because the library's other half must stay testable without a WebView. `cargo test --no-default-features` compiles
//! this crate with no `tauri` in the graph at all, and this module is the only thing that would break that — so it is
//! the only thing gated. The same reasoning as `build.rs`'s, and the two stay in step because both name the same
//! feature rather than each other.

// ─────────────────────────────────────────────────────────────────────────────
// The wrappers
// ─────────────────────────────────────────────────────────────────────────────
//
// ★ Why each command is written twice.
//
// `#[tauri::command]` generates a module (`__cmd__identity_get`) and a symbol per command, and it does that by
// referring to `tauri::State`. The library's command bodies deliberately do not depend on `tauri` — the
// `--no-default-features` build runs them with no WebView in the build graph at all — so the bodies in `commands::*`
// are plain functions and the attribute has to be applied out here, in the one place that is only built when the shell
// is.
//
// The wrappers are as thin as the signature allows: take the arguments, call, return what came back. None of them
// decides anything, because a decision made here is a decision no test can reach.
//
// ★ Why the dispatch table is also in this module, and not next to the builder.
//
// Because `#[tauri::command]` on a `pub fn` writes its wrapper macro with `#[macro_export]`, which puts that macro at
// the crate root **instead of** leaving it in textual scope here. `generate_handler!` expands to an unqualified
// `__cmd__identity_get!(…)`, so it resolves only where the macro's textual scope still reaches — inside this module,
// below each definition. One module above, the same line fails with "cannot find macro". The alternative was forty
// explicit macro imports; this is one function.
//
// ★ `needless_pass_by_value` is switched off for this block, and it is the one place in the crate where that is right.
//
// Every argument here is owned because `#[tauri::command]` deserialises it that way — the macro writes the
// `Deserialize` bound into the generated `__cmd__*` module, so `&str` is not an option without changing what the
// command accepts over the wire. The lint is correct everywhere else in this repository and wrong in exactly this
// block, which is the distinction an `allow` is for.

#[cfg(all(feature = "official-plugin-tests", debug_assertions, not(mobile)))]
mod official_smoke;

mod builder;
mod clock;
mod discovery;
mod package;
mod plugins;
mod runtime;
mod startup;
mod wire;
mod wrappers;

#[cfg(test)]
mod tests;

#[cfg(test)]
use builder::WAKE_SHORTCUT;
pub use builder::builder;
#[cfg(mobile)]
pub use runtime::run;
pub use runtime::run_window;
