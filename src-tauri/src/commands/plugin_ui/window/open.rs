use super::*;

/// A Tauri-safe package prefix; each document appends an independent, non-secret id.
#[must_use]
pub fn window_label_prefix(id: &synra_protocol::ids::PluginId) -> String {
    origin::label(id)
}

/// What opening a page decided.
///
/// ★ The decision is here rather than in `shell.rs` because the check and the reservation have to happen under one lock:
/// two fast clicks would otherwise both see no window, both issue a token, and the second would replace the first entry in
/// `hosted_ui` — leaving the first page holding a nonce no table names.
#[derive(Debug)]
pub enum OpenDecision {
    /// No page is open; the caller builds it with this request.
    Open(OpenRequest),
    /// A page is already open (or is being opened); the caller shows the window instead of building one.
    AlreadyOpen {
        /// The live window to focus.
        label: String,
    },
}

/// Prepares the page for one plugin, and records who it belongs to.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when there is no manifest to read, [`ErrorCode::Unsupported`] when the plugin has no
/// UI half. [`ErrorCode::CapabilityDenied`] rejects disabled, untrusted or concurrently changed authority.
pub fn open_request(state: &AppState, plugin_id: &str) -> Result<OpenDecision, IpcError> {
    use synra_protocol::ErrorCode;

    let id = crate::commands::plugins::parse(plugin_id)?;
    let manifest =
        crate::commands::plugins::read_manifest(state, &id.to_string()).ok_or_else(|| {
            IpcError::unknown(ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id}))
        })?;
    let Some(ui) = manifest.runtime.ui.as_ref() else {
        return Err(IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": "it has no UI"})));
    };

    let snapshot = state
        .db()
        .plugin_ui_snapshot(id.as_str())
        .map_err(internal)?
        .filter(|snapshot| snapshot.authority.permits_ui())
        .ok_or_else(authority::refused)?;
    let authority = snapshot.authority;
    let decisions = snapshot.grants;
    let existing = {
        let hosted = state.hosted_ui.lock();
        hosted
            .iter()
            .find(|(_, entry)| entry.plugin_id == id && entry.authority == authority)
            .map(|(label, entry)| (label.clone(), entry.clone()))
    };
    if let Some((label, entry)) = existing
        && self::authority::validate(state, &entry).is_ok()
    {
        return Ok(OpenDecision::AlreadyOpen { label });
    }
    let nonce = uuid::Uuid::new_v4().as_u128();
    // A public window label must not reveal the private RPC nonce.
    let label = format!(
        "{}-{}",
        window_label_prefix(&id),
        uuid::Uuid::new_v4().simple()
    );
    let url = origin::entry(&id, &ui.entry, ui.default_page.as_deref())?;
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
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
    let grants = synra_capability::CapabilitySet::parse_all(granted.iter().map(String::as_str))
        .map_err(|error| {
            // A decision the user made names a capability this build cannot parse, so the page's permissions are unknown.
            // Opening it with a guessed set would be worse than refusing: the plugin would run with permissions nobody chose.
            IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
                "pluginId": plugin_id,
                "reason": format!("a recorded grant is not a capability this build knows: {error}"),
            }))
        })?;
    // ★ The environment the page's façade answers from. These are the host's facts, decided once, before the page
    // loads: a round trip to learn something the shell already holds is a round trip that can fail, and a render that has
    // to await `env.platform()` is a render that cannot branch synchronously.
    let config = crate::commands::config::config_get(state)?;
    let locale = crate::locale::from_preference(
        config
            .values
            .get("ui.locale")
            .and_then(serde_json::Value::as_str),
    )
    .code()
    .to_owned();
    let dark_mode = config
        .values
        .get("ui.theme")
        .and_then(serde_json::Value::as_str)
        == Some("dark");
    let sdk_version = synra_plugin_host::SDK_VERSION;
    let platform = platform_name()?;
    let host_kind = if cfg!(any(target_os = "android", target_os = "ios")) {
        "mobile"
    } else {
        "desktop"
    };
    // ★ The platform's own answer, taken once. `supported_actions` is the askable list rather than a guess: it leaves
    // out `simulate_keys` when the policy forbids it, so a button built on this answer cannot offer something the host
    // would refuse.
    let platform_capabilities: Vec<String> = state
        .platform
        .supported_actions()
        .into_iter()
        .filter_map(|action| platform_capability(action).map(str::to_owned))
        .chain(synra_platform::pointer::supported().then(|| "control_pointer".to_owned()))
        .collect();

    let seed = InjectionSeed {
        label: &label,
        token: nonce.to_string(),
        plugin_id: id.as_str(),
        granted: &granted,
        denied_capabilities: &denied,
        locale: &locale,
        dark_mode,
        sdk_version,
        platform,
        host_kind,
        app_version: env!("CARGO_PKG_VERSION"),
        platform_capabilities,
    };
    let script = initialization_script(FACADE, &seed)?;

    if state.db().plugin_authority(id.as_str()).map_err(internal)? != Some(authority) {
        return Err(self::authority::refused());
    }
    let mut hosted = state.hosted_ui.lock();
    if let Some((label, _)) = hosted.iter().find(|(_, entry)| {
        entry.plugin_id == id
            && entry.authority == authority
            && state
                .ui_sessions
                .validate(entry.nonce, &id, &entry.version, now)
                .is_ok()
    }) {
        return Ok(OpenDecision::AlreadyOpen {
            label: label.clone(),
        });
    }
    let superseded = hosted
        .iter()
        .filter(|(_, entry)| entry.plugin_id == id)
        .map(|(label, _)| label.clone())
        .collect();
    state
        .ui_sessions
        .issue_with(&id, manifest.version.clone(), grants, nonce, now);
    hosted.insert(
        label.clone(),
        crate::state::HostedUi {
            plugin_id: id,
            version: manifest.version,
            nonce,
            ready: false,
            authority,
        },
    );
    drop(hosted);

    Ok(OpenDecision::Open(OpenRequest {
        label,
        url,
        script,
        superseded,
    }))
}
