//! The plugin page's two calls: "I have drawn" and "do this for me".
//!
//! # Why one takes a label and the other a token
//!
//! `plugin_ui_ready` is the page's first word, and it presents the label the host gave its window: `validate` needs the
//! nonce, the claiming plugin and the version, and only the host's own table can supply the last two. The token is still
//! checked — it is the entry's own nonce.
//!
//! `plugin_ui_rpc` presents the **token**, because a label is not a secret: labels are deterministic (`plugin-<id>`), so a
//! page could guess another plugin's. The host finds the entry whose nonce matches the presented token, then validates the
//! pair; a token no hosted page holds is refused before any method is looked at.
//!
//! # What it refuses, and what it deliberately does not say
//!
//! A token nobody holds, and a token the registry rejects, are both refusals with the same code and the same parameters.
//! Saying which of the two applies would let a page probe for values that exist — the enumeration a token is supposed to
//! prevent.

use crate::state::AppState;
use synra_ipc::{IpcError, PluginUiOp};
use synra_protocol::ErrorCode;

/// Records that a plugin page has mounted, after checking it holds a live session.
///
/// # Errors
///
/// [`ErrorCode::CapabilityDenied`] when the label is not one this shell hosted, or when the registry refuses the token —
/// see the module docs for why the two are not distinguished. [`ErrorCode::Internal`] is not refused here: validation does
/// not touch storage.
pub fn plugin_ui_ready(state: &AppState, label: &str) -> Result<(), IpcError> {
    let refused = || {
        IpcError::unknown(ErrorCode::CapabilityDenied)
            .with_params(serde_json::json!({"reason": "this page holds no live session"}))
    };

    // The three facts are **copied out** and the table's lock released before validation: `validate` takes the registry's
    // own lock, and holding two locks while a third party could take them in the other order is how a deadlock is built.
    // The window between the copy and the write is safe because the only thing that removes an entry is the window that
    // created it, and that same window is the one blocked on this command.
    let copy = {
        let hosted = state.hosted_ui.lock();
        hosted
            .get(label)
            .map(|entry| (entry.nonce, entry.plugin_id.clone(), entry.version.clone()))
    };
    let Some((nonce, plugin_id, version)) = copy else {
        return Err(refused());
    };

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    state.ui_sessions.validate(nonce, &plugin_id, &version, now).map_err(|_| refused())?;

    if let Some(entry) = state.hosted_ui.lock().get_mut(label) {
        entry.ready = true;
    }
    Ok(())
}

/// Answers one call from a plugin page, after checking the token it presents.
///
/// # Why the token alone is enough to find the page
///
/// Because the host issued it: [`open_request`] records the nonce beside the plugin it opened the window for, and this
/// looks the entry up by that nonce. The page never gets to say which plugin it is — it presents a value only the host
/// could have given it, and the host remembers the rest.
///
/// # What is implemented, and what is only addressable
///
/// `device.list` is the first operation with a body: it answers the same cards the devices screen shows, reduced to the
/// four fields the plugin contract declares. Every other operation on the surface answers [`ErrorCode::Unsupported`] —
/// an honest "this build does not do that yet", rather than an empty list or a promise nothing keeps.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] when the token is not a decimal number,
/// [`ErrorCode::CapabilityDenied`] when no hosted page holds it or the session is stale,
/// [`ErrorCode::Unsupported`] for a method that is not on the plugin surface or is not built here.
pub fn plugin_ui_rpc(
    state: &AppState,
    token: &str,
    method: &str,
    _args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let refused = || {
        IpcError::unknown(ErrorCode::CapabilityDenied)
            .with_params(serde_json::json!({"reason": "this page holds no live session"}))
    };

    let nonce = token.parse::<u128>().map_err(|_| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": "the token is not a number"}))
    })?;

    // The same copy-then-release shape as `plugin_ui_ready`: `validate` takes the registry's lock, and holding the two
    // in one expression is how a deadlock is built.
    let copy = {
        let hosted = state.hosted_ui.lock();
        hosted
            .values()
            .find(|entry| entry.nonce == nonce)
            .map(|entry| (entry.plugin_id.clone(), entry.version.clone()))
    };
    let Some((plugin_id, version)) = copy else {
        return Err(refused());
    };

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    state
        .ui_sessions
        .validate(nonce, &plugin_id, &version, now)
        .map_err(|_| refused())?;

    let Some(op) = PluginUiOp::parse(method) else {
        return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "method": method,
            "reason": "this host has no such operation",
        })));
    };

    match op {
        PluginUiOp::DeviceList => device_targets(state),
        _ => Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "method": method,
            "reason": "the operation is on the plugin surface and this build does not implement it",
        }))),
    }
}

/// The device list as a plugin page sees it: the four fields the client contract declares.
///
/// # Why the reduction happens here
///
/// Because the plugin's `DeviceTarget` is a **different type** from the host's `DeviceCard` on purpose: a card carries a
/// transfer summary, an address seen on the network, a blocked flag — things a plugin has no business knowing. Mapping in
/// the interface would put that judgement in JavaScript; mapping here means the page cannot receive what it may not see.
fn device_targets(state: &AppState) -> Result<serde_json::Value, IpcError> {
    let snapshot = super::devices::devices_snapshot(state)?;
    let targets: Vec<serde_json::Value> = snapshot
        .devices
        .iter()
        .map(|card| {
            serde_json::json!({
                "deviceId": card.device_id.to_string(),
                "displayName": card.display_name,
                "platform": card.platform,
                "online": card.online,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(targets))
}

/// The façade a plugin page runs: built by `pnpm run build:facade`, embedded here.
///
/// # Why the built file is committed
///
/// Because a Rust build must not need Node. The alternative — generating it in the Rust build — would make `cargo check`
/// depend on pnpm, and a contributor without the frontend toolchain could not compile the shell. The file is generated
/// and checked instead: `scripts/check-shapes.ts` rebuilds it and fails if it disagrees with this one, so it cannot drift
/// from `src/ipc/injection/bootstrap.ts` without a red gate.
const FACADE: &str = include_str!("../../inject/facade.js");

/// The facts the shell writes into a plugin page before any of its own scripts run.
///
/// # Why a JSON object rather than a series of assignments
///
/// Because the façade reads **all** of it, and a missing field would leave a namespace answering `undefined` — the exact
/// class of failure the injected-surface tests exist to prevent. Serialising one struct means the Rust side and
/// `InjectionSeed` in `bootstrap.ts` are one shape, and a change to either is visible as a diff to the other.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InjectionSeed<'a> {
    label: &'a str,
    token: String,
    plugin_id: &'a str,
    granted: &'a [String],
    denied_capabilities: &'a [String],
    locale: &'a str,
    dark_mode: bool,
    sdk_version: &'a str,
    platform: &'a str,
    host_kind: &'a str,
    app_version: &'a str,
}

/// The script a plugin page is started with: the seed, then the façade that reads it.
///
/// # Why the token is a **string**
///
/// Because it is a `u128`, and JavaScript numbers are exact only to 2^53. A nonce that silently loses its low digits is a
/// token that validates for nobody, and the failure would look like "the page never became ready" rather than like a
/// rounding error.
///
/// # Why `Object.freeze` before the façade runs
///
/// The page runs someone else's code. A mutable seed is one a script can rewrite before the façade reads it — and a page
/// that can rewrite its own `granted` list can make `env.can` answer true for a capability the user refused. The façade
/// copies the values into its closure, so later writes to the global would not affect it either.
///
/// # Why the façade is appended rather than imported
///
/// Because an initialization script has no module loader: Tauri evaluates one script before the page's own, and an
/// `import()` would resolve after the plugin's first line has already run. The built file is an IIFE, so it executes in
/// place and `window.synra` exists by the time the page's code starts.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the seed cannot be serialised, which for these field types cannot happen — and is an error
/// rather than an `unwrap` because the workspace denies the panic.
fn initialization_script(facade: &str, seed: &InjectionSeed<'_>) -> Result<String, IpcError> {
    let json = serde_json::to_string(seed).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    Ok(format!(
        "Object.freeze(window.__SYNRA_PLUGIN__ = Object.freeze({json}));\n{facade}"
    ))
}

/// The platform name the plugin contract uses.
///
/// # Why an unknown platform is a refusal
///
/// Because the client's `Platform` is a closed union of five values, and a sixth name would be a value a plugin's type
/// says is impossible. A plugin that branches on the platform would take the `default` path on a machine this host does
/// not have a name for — a plausible wrong answer, which is worse than "this page cannot open here".
fn platform_name() -> Result<&'static str, IpcError> {
    Ok(match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        other => {
            return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "platform": other,
                "reason": "the plugin contract has no name for this platform",
            })))
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
    /// The WebView's label, and the key `plugin_ui_ready` is called with.
    pub label: String,
    /// The page's address, under the plugin origin: `synra-plugin://<plugin-id>/<page>`.
    pub url: String,
    /// The initialization script carrying the token and the label.
    pub script: String,
}

/// Prepares the page for one plugin, and records who it belongs to.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when there is no manifest to read, [`ErrorCode::Unsupported`] when the plugin has no
/// UI half. Both are refusals a caller can act on: the first says "install it", the second says "this one has no window".
pub fn open_request(state: &AppState, plugin_id: &str) -> Result<OpenRequest, IpcError> {
    use synra_protocol::ErrorCode;

    let id = super::plugins::parse(plugin_id)?;
    let manifest = super::plugins::read_manifest(state, &id.to_string()).ok_or_else(|| {
        IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id}))
    })?;
    let Some(ui) = manifest.runtime.ui.as_ref() else {
        return Err(IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": "it has no UI"})));
    };

    // ★ The page key is the manifest's default page when it names one, and the entry's file stem otherwise: the address
    // shape is `synra-plugin://<plugin-id>/<page>` and `entry` is a path inside the bundle (`dist/index.html`), which is
    // not a page key.
    let page = ui.default_page.clone().unwrap_or_else(|| {
        std::path::Path::new(&ui.entry)
            .file_stem()
            .map_or_else(|| "index".to_owned(), |stem| stem.to_string_lossy().into_owned())
    });
    let label = format!("plugin-{}", id.as_str().replace('/', "__"));
    let url = format!("synra-plugin://{}/{page}", id.as_str());

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    // ★ The grants the page will hold, read from the decisions the user actually made. An empty set here made every
    // page powerless no matter what it had been granted — `env.can` would answer false for everything, and a gate built
    // on the session would refuse calls the user had allowed.
    let decisions = state.db().plugin_grants(id.as_str()).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let granted: Vec<String> = decisions
        .iter()
        .filter(|row| row.granted)
        .map(|row| row.capability.clone())
        .collect();
    let denied: Vec<String> = decisions
        .iter()
        .filter(|row| !row.granted)
        .map(|row| row.capability.clone())
        .collect();
    let grants = synra_capability::CapabilitySet::parse_all(granted.iter().map(String::as_str)).map_err(|error| {
        // A decision the user made names a capability this build cannot parse, so the page's permissions are unknown.
        // Opening it with a guessed set would be worse than refusing: the plugin would run with permissions nobody chose.
        IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "reason": format!("a recorded grant is not a capability this build knows: {error}"),
        }))
    })?;
    // ★ A random nonce. `issue_placeholder` is a counter — its own documentation says it is not for production — and a
    // page that can guess another page's token can call as that plugin. The window is the one place a real nonce can be
    // made, so it is made here.
    let nonce = state.ui_sessions.issue_with(
        &id,
        manifest.version.clone(),
        grants,
        uuid::Uuid::new_v4().as_u128(),
        now,
    );

    // ★ The environment the page's façade answers from. These are the host's facts, decided once, before the page
    // loads: a round trip to learn something the shell already holds is a round trip that can fail, and a render that has
    // to await `env.platform()` is a render that cannot branch synchronously.
    let config = super::config::config_get(state)?;
    let locale = config
        .values
        .get("ui.locale")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("en")
        .to_owned();
    let dark_mode = config.values.get("ui.theme").and_then(serde_json::Value::as_str) == Some("dark");
    // The host the plugin was built against, which is what `env.sdkVersion()` answers; `min` is the compatibility floor,
    // and reporting it would tell the plugin it was built against an older host than it was.
    let sdk_version = manifest
        .sdk
        .preferred
        .clone()
        .unwrap_or_else(|| manifest.sdk.min.clone());
    let platform = platform_name()?;
    let host_kind = if cfg!(any(target_os = "android", target_os = "ios")) {
        "mobile"
    }
    else {
        "desktop"
    };

    let seed = InjectionSeed {
        label: &label,
        token: nonce.to_string(),
        plugin_id: id.as_str(),
        granted: &granted,
        denied_capabilities: &denied,
        locale: &locale,
        dark_mode,
        sdk_version: &sdk_version,
        platform,
        host_kind,
        app_version: env!("CARGO_PKG_VERSION"),
    };
    let script = initialization_script(FACADE, &seed)?;

    state.hosted_ui.lock().insert(
        label.clone(),
        crate::state::HostedUi { plugin_id: id, version: manifest.version, nonce, ready: false },
    );

    Ok(OpenRequest { label, url, script })
}

/// One file out of a plugin's extracted UI bundle.
///
/// # What this is, and what it is not
///
/// It is the **policy** half of the `synra-plugin://` handler: which paths are legal, which content type a file gets, and
/// that the joined path is still inside the bundle. It is not the Tauri handler, because a handler needs a running
/// application and this needs a directory — so the interesting failure (a page asking for `../../identity.toml`) is a unit
/// test rather than a manual experiment.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path the artifact rules refuse or one that escapes the root, and
/// [`ErrorCode::Internal`] when the file cannot be read. The two are different answers: the first is somebody's bug or an
/// attack, the second is a broken install.
pub fn ui_file(root: &std::path::Path, path: &str) -> Result<(Vec<u8>, &'static str), IpcError> {
    use synra_protocol::ErrorCode;

    synra_plugin_ui::artifact::check_path(path).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;

    let file = root.join(path);
    // ★ Belt **and** braces: `check_path` already refuses `..` and absolute paths, and this refuses a join that escaped
    // anyway. A path check that is the only guard is one refactor away from not being a guard.
    if !synra_store::is_within(root, &file) {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": "it leaves the bundle"})));
    }

    let bytes = std::fs::read(&file).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;
    // A file nobody said the type of is still a file: `application/octet-stream` makes the WebView download it rather than
    // guess, which is the honest default.
    let content_type = synra_plugin_ui::artifact::content_type_for(path).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

/// Splits a `synra-plugin://` address into the plugin it names and the path inside its bundle.
///
/// # Why this is not a `Url` parse
///
/// A plugin id contains a slash (`@acme/hello`), and the address is `synra-plugin://<plugin-id>/<path>` — so a URL parser
/// would read `@acme` as the host and `/hello/index.html` as the path, splitting the id in the wrong place. The split is
/// therefore made against the ids the host actually opened: the **longest** one that prefixes the address wins, and what
/// follows is the path.
///
/// Longest rather than first-match because two ids can share a prefix (`@acme/hello` and `@acme/hello-world`): first-match
/// would serve one plugin's files under the other's name.
#[must_use]
pub fn split_ui_url<'a>(url: &str, known: &'a [synra_protocol::ids::PluginId]) -> Option<(&'a synra_protocol::ids::PluginId, String)> {
    const SCHEME: &str = "synra-plugin://";
    let rest = url.strip_prefix(SCHEME)?;
    // A query or fragment is not part of a file path, and a cache-busting `?v=1` is common enough to allow.
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);

    let mut best: Option<(&'a synra_protocol::ids::PluginId, String)> = None;
    for id in known {
        let Some(after) = rest.strip_prefix(id.as_str()) else {
            continue;
        };
        let Some(path) = after.strip_prefix('/') else {
            continue;
        };
        let better = best.as_ref().is_none_or(|(winner, _)| id.as_str().len() > winner.as_str().len());
        if better {
            best = Some((id, path.to_owned()));
        }
    }
    best
}

/// The `synra-plugin://` handler: one bundle file, or a refusal.
///
/// # Why it is here and not in `shell.rs`
///
/// Because it is three decisions and one call: which ids a request may name, which file it may have, and what to answer
/// when the answer is no. Those are the same decisions [`split_ui_url`] and [`ui_file`] make, and they have tests; the
/// protocol registration in `shell.rs` is wiring.
#[cfg(feature = "tauri-shell")]
pub fn serve(
    app: &tauri::AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    use tauri::Manager as _;

    fn answer(status: u16, body: &'static str) -> tauri::http::Response<Cow<'static, [u8]>> {
        tauri::http::Response::builder()
            .status(status)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Cow::Borrowed(body.as_bytes()))
            .unwrap_or_else(|_| tauri::http::Response::new(Cow::Borrowed(b"the response could not be built")))
    }

    let state = app.state::<AppState>();
    let uri = request.uri().to_string();

    // ★ The ids come from the host's own table, not from the request: a page may only name a plugin this shell opened a
    // window for, which is what makes the longest-prefix split safe.
    let known: Vec<synra_protocol::ids::PluginId> =
        state.hosted_ui.lock().values().map(|entry| entry.plugin_id.clone()).collect();

    let Some((plugin_id, path)) = split_ui_url(&uri, &known) else {
        return answer(404, "no page by that name");
    };
    let root = synra_plugin_ui::artifact::bundle_dir(&state.paths.plugin_dir(plugin_id.as_str()));

    match ui_file(&root, &path) {
        Ok((bytes, content_type)) => tauri::http::Response::builder()
            .status(200)
            .header(tauri::http::header::CONTENT_TYPE, content_type)
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| answer(500, "the response could not be built")),
        Err(refusal) => {
            // 403 for a path the rules refuse, 404 for a file that is not there: the first is somebody asking for something
            // they may never have, the second is a bundle that is missing a file it should carry.
            let status = if refusal.code == synra_protocol::ErrorCode::InvalidArgument { 403 } else { 404 };
            answer(status, "no page by that name")
        }
    }
}

#[cfg(test)]
mod tests;
