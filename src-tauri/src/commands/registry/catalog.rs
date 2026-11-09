use super::*;

/// What the npm search endpoint answers.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchResponse {
    #[serde(default)]
    objects: Vec<SearchObject>,
}

/// One result's envelope.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchObject {
    package: SearchPackage,
}

/// The package fields a result carries.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchPackage {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    publisher: Option<SearchPublisher>,
}

/// Who published the package, as the search endpoint reports it.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchPublisher {
    #[serde(default)]
    username: Option<String>,
}

/// The most results a caller may receive, whatever it asked for.
pub(crate) const MAX_SEARCH_RESULTS: u32 = 50;

/// How many results a search returns when the caller names no limit.
pub(crate) const DEFAULT_SEARCH_RESULTS: u32 = 20;

/// Searches the configured registry's npm-compatible search endpoint.
///
/// The query is scoped to `keywords:synra-plugin` so an ordinary npm package is not offered as a plugin. Results whose
/// name is not a plugin id are dropped rather than shown as something this device could install.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, and a fetch refusal's own code for a network problem.
pub fn search(
    state: &AppState,
    query: &CatalogQuery,
) -> Result<Vec<CatalogItem>, synra_ipc::IpcError> {
    let bases: Vec<Base> = bases(state)?
        .into_iter()
        .filter(|base| base.kind == Kind::Npm)
        .collect();
    if bases.is_empty() {
        // A static index is one document per package; there is no index to search, and a request to one would be a 404
        // dressed up as "nothing matched".
        return Err(
            synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "why": "the configured registries serve no search endpoint",
            })),
        );
    }
    let text = query.query.trim();
    let scope = if text.is_empty() {
        "keywords:synra-plugin".to_owned()
    } else {
        format!("keywords:synra-plugin {text}")
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEARCH_RESULTS)
        .clamp(1, MAX_SEARCH_RESULTS);
    let offset = query.offset.unwrap_or(0);
    let path = format!(
        "/-/v1/search?text={}&size={limit}&from={offset}",
        encode(&scope)
    );
    let (_, response) = get_text(&bases, |_| path.clone(), "the search endpoint")?;
    let parsed: SearchResponse = serde_json::from_str(&response.body).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })?;
    let installed: std::collections::BTreeSet<String> = state
        .db()
        .plugins()
        .map(|rows| rows.into_iter().map(|row| row.plugin_id).collect())
        .unwrap_or_default();
    let mut items: Vec<CatalogItem> = parsed
        .objects
        .into_iter()
        .filter_map(|object| {
            let package = object.package;
            let plugin_id = PluginId::try_from(package.name.clone()).ok()?;
            Some(CatalogItem {
                plugin_id,
                title: package.name.clone(),
                description: package.description,
                version: package.version,
                publisher: package
                    .publisher
                    .and_then(|publisher| publisher.username)
                    .unwrap_or_default(),
                // The search endpoint does not report a manifest, so the count is unknown here; the preview shows the
                // real list before anything is installed.
                capability_count: 0,
                installed: installed.contains(&package.name),
            })
        })
        .collect();
    // ★ Sorting happens here because npm's index has no `sort` parameter: relevance is whatever order it returned, and
    // name is a comparison this side can make honestly. A sort the response cannot support is not offered (see
    // `CatalogSort`), rather than silently returning relevance under another name.
    if query.sort == Some(CatalogSort::Name) {
        items.sort_by_key(|item| item.title.to_lowercase());
    }
    items.truncate(limit as usize);
    Ok(items)
}

/// Percent-encodes a query string for a URL.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte))
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A parsed registry document, and what a static directory claims about its publishers.
pub(crate) struct RegistryDocument {
    /// Which registry answered, so the tarball can be fetched from it or its mirrors.
    pub(crate) base: Base,
    /// The metadata, in the shape the install reads.
    pub(crate) packument: Packument,
    /// Publisher id to fingerprint, from a static index; empty for an npm registry.
    pub(crate) publishers: std::collections::BTreeMap<String, String>,
    /// The static index's own URL, which a relative tarball is resolved against.
    pub(crate) index_url: Option<String>,
}

/// The packument for one package.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or serves
/// something that is not a packument, and a fetch refusal's own code for a network problem.
pub fn packument(state: &AppState, plugin_id: &str) -> Result<Packument, synra_ipc::IpcError> {
    Ok(packument_with_base(state, plugin_id)?.packument)
}

/// Whether the registry publishes a version newer than the one installed, and what it asks for that the old one did not.
///
/// # What is compared, and what is believed
///
/// The published version string and the installed manifest's capabilities — the packument's inline manifest is the
/// publisher's claim about the new version, and this answer is a prompt to look, not a decision: the install still verifies
/// the bytes and the preview still shows the real permissions.
pub fn check_update(
    state: &AppState,
    plugin_id: &str,
) -> Result<Option<UpdateInfo>, synra_ipc::IpcError> {
    check_update_at(state, plugin_id, synra_store::sqlite::now_ms())
}

pub(crate) fn check_update_at(
    state: &AppState,
    plugin_id: &str,
    now_ms: i64,
) -> Result<Option<UpdateInfo>, synra_ipc::IpcError> {
    // Capture the source before fetching. The store checks it again in the write, so changing registries while a
    // request is in flight cannot publish an answer from the previous source.
    let registry = state
        .db()
        .config()
        .map_err(|error| {
            synra_ipc::IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"detail": error.to_string()}))
        })?
        .into_iter()
        .find(|row| row.key == REGISTRY_KEY)
        .map(|row| row.value);
    let plugin = synra_protocol::PluginId::parse(plugin_id).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })?;
    let active = state.db().active_plugin(plugin_id).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })?;
    let Some(active) = active else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id})));
    };
    let document = packument(state, plugin_id)?;
    let latest = document
        .latest()
        .filter(|latest| synra_registry::is_newer(&latest.version, &active.version));
    let Some(latest) = latest else {
        return crate::commands::plugin_updates::record(
            state,
            &synra_store::PluginUpdateCheck {
                plugin_id: plugin.to_string(),
                current_version: active.version,
                checked_at_ms: now_ms,
                registry,
                available_version: None,
                added_capabilities: Vec::new(),
                attempts: 0,
                retry_at_ms: None,
            },
        );
    };
    let installed = installed_capabilities(state, plugin_id);
    let published: Vec<String> = latest
        .synra
        .as_ref()
        .map(|manifest| {
            manifest
                .capabilities
                .iter()
                .map(|declaration| declaration.cap.clone())
                .collect()
        })
        .unwrap_or_default();
    let added: Vec<String> = published
        .into_iter()
        .filter(|capability| !installed.contains(capability))
        .collect();
    crate::commands::plugin_updates::record(
        state,
        &synra_store::PluginUpdateCheck {
            plugin_id: plugin.to_string(),
            current_version: active.version,
            checked_at_ms: now_ms,
            registry,
            available_version: Some(latest.version.clone()),
            added_capabilities: added,
            attempts: 0,
            retry_at_ms: None,
        },
    )
}

/// The capabilities in the manifest this device installed, read from the plugin's own directory.
///
/// A missing or unreadable manifest answers an empty list rather than failing the check: an update that cannot be compared
/// is still an update, and refusing to say so would hide the version bump behind a parse error.
pub(crate) fn installed_capabilities(state: &AppState, plugin_id: &str) -> Vec<String> {
    // ★ `plugin_dir`, not a raw join: a scoped id contains a separator, and the directory the installer writes is the
    // sanitised one. Joining the id here looked for `plugins/@acme/hello` while the artifact was at `plugins/acme+hello`,
    // so every update of a scoped plugin was compared against an empty capability set.
    let path = state.paths.plugin_dir(plugin_id).join("manifest.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<synra_plugin_api::Manifest>(&text).ok())
        .map(|manifest| {
            manifest
                .capabilities
                .into_iter()
                .map(|declaration| declaration.cap)
                .collect()
        })
        .unwrap_or_default()
}
