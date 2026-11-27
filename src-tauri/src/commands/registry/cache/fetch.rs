use super::*;

/// The packument and the mirror that served it.
///
/// The mirror matters because the tarball it names may only be on its origin; the install uses it to decide which other
/// mirrors may serve the same path.
pub(crate) fn packument_with_base(
    state: &AppState,
    plugin_id: &str,
) -> Result<RegistryDocument, synra_ipc::IpcError> {
    let bases = bases(state)?;
    packument_from_sources(state, plugin_id, &bases)
}

pub(crate) fn packument_from_sources(
    state: &AppState,
    plugin_id: &str,
    bases: &[Base],
) -> Result<RegistryDocument, synra_ipc::IpcError> {
    let mut last = None;

    for base in bases {
        let path = match base.kind() {
            Kind::Npm => format!("/{}", plugin_id.replace('/', "%2F")),
            Kind::Static => format!("/{plugin_id}.json"),
        };
        let url = format!("{}{path}", base.url());

        // ★ The cached document and its validators are cloned before the lock is dropped: a conditional request spends a
        // network round trip, and a mutex held across it would make every other registry read wait for the slowest bucket.
        // An entry whose `max-age` has run out is not sent back as a conditional candidate — the host said how long it
        // was good for, and asking it to revalidate something this side already knows is stale would be a second opinion.
        let now = synra_store::sqlite::now_ms();
        let mut cached = state
            .registry_cache
            .lock()
            .get(&url)
            .filter(|entry| entry.expires_at_ms.is_none_or(|expires| expires > now))
            .cloned();
        // ★ A miss in this process is not a miss on this device: the previous run may have written the document and its
        // validators. Reading it back turns "open the app, check every plugin" from N fetches into N conditional requests.
        if cached.is_none()
            && let Some(loaded) = load_document(state, &url, base.kind(), now)
        {
            state
                .registry_cache
                .lock()
                .insert(url.clone(), loaded.clone());
            cached = Some(loaded);
        }
        let mut request = get(&url);
        if let Some(entry) = &cached {
            if let Some(etag) = &entry.etag {
                request
                    .headers
                    .insert("if-none-match".to_owned(), etag.clone());
            }
            if let Some(modified) = &entry.last_modified {
                request
                    .headers
                    .insert("if-modified-since".to_owned(), modified.clone());
            }
        }

        match client(base).fetch(&request) {
            Ok(response) if response.status == 304 => {
                // A 304 says "what you have is current". Without a cached document there is nothing to have, so a host
                // that answers one anyway is refused rather than guessed about.
                if let Some(mut entry) = cached {
                    if let Some(age) = max_age_ms(&response.headers) {
                        entry.expires_at_ms = Some(now.saturating_add(age));
                        state
                            .registry_cache
                            .lock()
                            .insert(url.clone(), entry.clone());
                    }
                    return Ok(RegistryDocument {
                        base: base.clone(),
                        packument: entry.document,
                        publishers: entry.publishers,
                        index_url: Some(url.clone()),
                    });
                }
                last = Some(synra_ipc::IpcError::unknown(ErrorCode::ProtocolViolation).with_params(
                    serde_json::json!({"url": url, "reason": "the registry answered 304 without a cached document"}),
                ));
            }
            Ok(response) if (200..300).contains(&response.status) => {
                let (document, publishers) = match base.kind() {
                    Kind::Npm => (
                        Packument::parse(&response.body).map_err(|error| {
                            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                                serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}),
                            )
                        })?,
                        std::collections::BTreeMap::new(),
                    ),
                    Kind::Static => {
                        let index = synra_registry::parse_static_index(&response.body).map_err(|error| {
                            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                                serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}),
                            )
                        })?;
                        (index.document, index.publishers)
                    }
                };
                // ★ An npm packument is cached too: the validators are the host's, and a check-update that hits `304`
                // costs one round trip instead of re-parsing a document that has not changed. `publishers` is empty for
                // npm, which is the honest value — only a static directory makes that claim.
                let expires_at_ms =
                    max_age_ms(&response.headers).map(|age| now.saturating_add(age));
                state.registry_cache.lock().insert(
                    url.clone(),
                    CachedDocument {
                        etag: response.headers.get("etag").cloned(),
                        last_modified: response.headers.get("last-modified").cloned(),
                        document: document.clone(),
                        publishers: publishers.clone(),
                        expires_at_ms,
                    },
                );
                // ★ The validators are what make the disk copy useful: without them the next process would have to fetch the
                // body again to learn whether it changed.
                store_document(
                    state,
                    &url,
                    base.kind(),
                    &response.body,
                    &response.headers,
                    expires_at_ms,
                );
                return Ok(RegistryDocument {
                    base: base.clone(),
                    packument: document,
                    publishers,
                    index_url: (base.kind() == Kind::Static).then(|| url.clone()),
                });
            }
            Ok(response) if retryable(response.status) => {
                last = Some(http_failure(response.status, &url, Operation::Metadata));
            }
            Ok(response) => {
                return Err(http_failure(response.status, &url, Operation::Metadata));
            }
            Err(refusal) => {
                last = Some(network_failure(refusal, &url));
            }
        }
    }

    Err(exhausted(last, Operation::Metadata))
}
