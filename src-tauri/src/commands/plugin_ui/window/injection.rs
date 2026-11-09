use super::super::{IpcError, origin};
use synra_protocol::ErrorCode;

/// The façade a plugin page runs: built by `pnpm run build:facade`, embedded here.
///
/// # Why the built file is committed
///
/// Because a Rust build must not need Node. The alternative — generating it in the Rust build — would make `cargo check`
/// depend on pnpm, and a contributor without the frontend toolchain could not compile the shell. The file is generated
/// and checked instead: `scripts/check-shapes.ts` rebuilds it and fails if it disagrees with this one, so it cannot drift
/// from `src/ipc/injection/bootstrap.ts` without a red gate.
pub(crate) const FACADE: &str = include_str!("../../../../inject/facade.js");

/// The facts the shell writes into a plugin page before any of its own scripts run.
///
/// # Why a JSON object rather than a series of assignments
///
/// Because the façade reads **all** of it, and a missing field would leave a namespace answering `undefined` — the exact
/// class of failure the injected-surface tests exist to prevent. Serialising one struct means the Rust side and
/// `InjectionSeed` in `bootstrap.ts` are one shape, and a change to either is visible as a diff to the other.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InjectionSeed<'a> {
    pub(crate) label: &'a str,
    pub(crate) token: String,
    pub(crate) plugin_id: &'a str,
    pub(crate) granted: &'a [String],
    pub(crate) denied_capabilities: &'a [String],
    pub(crate) locale: &'a str,
    pub(crate) dark_mode: bool,
    pub(crate) sdk_version: &'a str,
    pub(crate) platform: &'a str,
    pub(crate) host_kind: &'a str,
    pub(crate) app_version: &'a str,
    /// The platform actions this host has, in the client's own spelling.
    ///
    /// ★ Separate from `granted`: this is the platform's answer ("can this machine show a notification?"), not the
    /// user's ("may this plugin show one?"). `env.platformSupports` is synchronous so a render can disable a button, so
    /// the list is decided before the page loads rather than asked for later.
    pub(crate) platform_capabilities: Vec<String>,
}

/// The client's name for a platform action.
///
/// # Why this is not `PlatformAction::capability`
///
/// Because they are different vocabularies on purpose: `platform:notify` is the permission the manifest declares and the
/// gate checks, while `notify` is the name `env.platformSupports` takes (the contract's `PlatformCapability` union). A
/// page asking "can this host notify?" is asking about the machine, not about its grant, and passing one spelling where
/// the other belongs would answer `false` for every action on a host that supports all of them.
pub(crate) fn platform_capability(action: synra_platform::PlatformAction) -> Option<&'static str> {
    use synra_platform::PlatformAction;

    Some(match action {
        PlatformAction::OpenUrl => "open_url",
        PlatformAction::OpenPath => "open_path",
        PlatformAction::RevealPath => "reveal_path",
        PlatformAction::SimulateKeys => "simulate_keys",
        PlatformAction::ReadClipboard => "clipboard_read",
        PlatformAction::WriteClipboard => "clipboard_write",
        PlatformAction::Notify => "notify",
        // `PlatformAction` is `non_exhaustive`: an action added to the platform before the client names it is left out of
        // the answer rather than guessed at. `env.platformSupports` then says `false`, which is the safe default for a
        // button — a capability this page does not know how to ask about is not one to offer.
        _ => return None,
    })
}

/// The script a plugin page is started with: the facade inside a function, with the seed as its argument.
///
/// # Why the token is a **string**
///
/// Because it is a `u128`, and JavaScript numbers are exact only to 2^53. A nonce that silently loses its low digits is a
/// token that validates for nobody, and the failure would look like "the page never became ready" rather than like a
/// rounding error.
///
/// # Why the seed is an argument rather than a global
///
/// The design document's rule is that the token lives in a closure, not on an object. A `window.__SYNRA_PLUGIN__` would
/// be readable by any script the plugin's page includes — an analytics bundle, a dependency with a bad day — and a token
/// that can be read is a session that can be used from somewhere else. Here the seed is the wrapper function's
/// parameter: it exists only for the lifetime of the call, the facade copies what it needs, and nothing can look it up
/// afterwards. A page script can still *use* `window.synra`, which is the intended surface.
///
/// # Why the facade is inline rather than imported
///
/// Because an initialization script has no module loader: Tauri evaluates one script before the page's own, and an
/// `import()` would resolve after the plugin's first line has already run. The built file is an IIFE, so `var
/// SynraInjection` is a local of the wrapper and `SynraInjection.boot(seed)` runs in place — `window.synra` exists by
/// the time the page's code starts.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the seed cannot be serialised, which for these field types cannot happen — and is an error
/// rather than an `unwrap` because the workspace denies the panic.
pub(crate) fn initialization_script(
    facade: &str,
    seed: &InjectionSeed<'_>,
) -> Result<String, IpcError> {
    let json = serde_json::to_string(seed).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    Ok(format!(
        "(function(seed){{{facade}\nSynraInjection.boot(seed);}})({json});"
    ))
}

/// The platform name the plugin contract uses.
///
/// # Why an unknown platform is a refusal
///
/// Because the client's `Platform` is a closed union of five values, and a sixth name would be a value a plugin's type
/// says is impossible. A plugin that branches on the platform would take the `default` path on a machine this host does
/// not have a name for — a plausible wrong answer, which is worse than "this page cannot open here".
pub(crate) fn platform_name() -> Result<&'static str, IpcError> {
    Ok(match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        other => {
            return Err(
                IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                    "platform": other,
                    "reason": "the plugin contract has no name for this platform",
                })),
            );
        }
    })
}

/// What the shell needs in order to open one plugin page.
///
/// The parts are separated from the Tauri call on purpose: a URL, a label and a script are values a test can assert, and
/// `WebviewWindowBuilder` is a call no test here can make. Everything that could be wrong in an interesting way is in this
/// struct; what is left in `shell.rs` is five lines of plumbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    /// Previous authority generations to destroy; their nonces are already unusable.
    pub superseded: Vec<String>,
    /// The WebView's label, and the key `plugin_ui_ready` is called with.
    pub label: String,
    /// The actual entry address under the package's URL-safe origin.
    pub url: String,
    /// The initialization script carrying the token and the label.
    pub script: String,
}

/// Whether a navigation stays on the page that was opened.
///
/// # Why the rule is "the exact page", not "the plugin's origin"
///
/// Because the two are not the same safety property. The page was opened with **this** plugin's token and grants, and
/// the initialization script is attached to the WebView, not to a URL — so a navigation to another page of the same
/// bundle would keep this page's authority while showing content the opener never chose. In-page routing (`#…`, the
/// history API) still works, which is how a single-page plugin is supposed to navigate; a document navigation to a
/// second address does not.
///
/// A fragment is part of the same document: `#/settings` is a hash route, not a different page. Everything else — another
/// path, another plugin's origin, `https:`, `file:`, `data:` — is refused.
///
/// The attribute is because only the `tauri-shell` build has a `WebviewWindowBuilder` to hand this to; the predicate stays
/// compiled in every build so its tests run where the WebView does not.
#[cfg_attr(not(feature = "tauri-shell"), allow(dead_code))]
pub(crate) fn stays_on_page(candidate: &str, opened: &str) -> bool {
    origin::stays(candidate, opened)
}
