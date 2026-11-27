use super::*;

/// The configured registries, in the order they are tried.
///
/// The key holds either one URL or an array of mirrors; an array is tried in order, and only for failures that another
/// mirror could plausibly fix (16 §4.2). A verification failure never falls back — it is not a network problem, and
/// retrying it somewhere else would be skipping the check the design exists to keep.
pub(crate) fn bases(state: &AppState) -> Result<Vec<Base>, synra_ipc::IpcError> {
    let setting = read_setting(state)?;
    bases_from_setting(setting.as_deref())
}

pub(crate) fn read_setting(state: &AppState) -> Result<Option<String>, synra_ipc::IpcError> {
    synra_core::plugin_updates::registry_setting(&state.db()).map_err(crate::state::internal)
}

pub(crate) fn bases_from_setting(setting: Option<&str>) -> Result<Vec<Base>, synra_ipc::IpcError> {
    let value = setting
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|error| invalid_configuration(None, error.to_string()))?;
    let config = validate_setting(value.as_ref())?;
    if config.sources().is_empty() {
        return Err(synra_ipc::RegistryProblem::NotConfigured.into_error());
    }
    Ok(config.sources().to_vec())
}

/// Validate a setting before committing it, and when reading existing configuration.
pub(crate) fn validate_setting(
    value: Option<&serde_json::Value>,
) -> Result<synra_registry::RegistryConfig, synra_ipc::IpcError> {
    synra_registry::RegistryConfig::parse(value)
        .map_err(|error| invalid_configuration(error.index, error.reason.to_string()))
}

fn invalid_configuration(index: Option<usize>, reason: String) -> synra_ipc::IpcError {
    synra_ipc::RegistryProblem::InvalidConfiguration {
        field: REGISTRY_KEY.to_owned(),
        index,
        reason,
    }
    .into_error()
}

/// Whether another mirror could plausibly fix this status.
///
/// ★ 404 and 429 are the design's two (16 §4.2), plus the gateway-shaped 5xx. A 401 or a 403 is a decision the registry
/// made about this client, and asking a second registry the same question would not change it.
pub(crate) fn retryable(status: u16) -> bool {
    matches!(status, 404 | 408 | 429 | 500..=599)
}

/// Fetches one path from the first mirror that answers usefully, returning the mirror and the response.
pub(crate) fn get_text(
    bases: &[Base],
    path_for: impl Fn(&Base) -> String,
    operation: Operation,
) -> Result<(Base, synra_fetch::Response), synra_ipc::IpcError> {
    let mut last = None;
    for base in bases {
        let path = path_for(base);
        let url = format!("{}{path}", base.url());
        match client(base).fetch(&get(&url)) {
            Ok(response) if (200..300).contains(&response.status) => {
                return Ok((base.clone(), response));
            }
            Ok(response) if retryable(response.status) => {
                last = Some(http_failure(response.status, &url, operation));
            }
            Ok(response) => {
                return Err(http_failure(response.status, &url, operation));
            }
            Err(refusal) => {
                last = Some(network_failure(refusal, &url));
            }
        }
    }
    Err(exhausted(last, operation))
}

/// The guarded HTTP client for one registry.
pub(crate) fn client(base: &Base) -> Client {
    guarded_client(
        NetworkGuard::for_hosts(&[base.host().to_owned()])
            .allowing_insecure(!base.secure())
            .with_policy(IpPolicy {
                allow_loopback: base.loopback(),
                ..IpPolicy::default()
            }),
    )
}

/// Artifact URLs use the fetch guard's parser. Their query is not a source configuration prefix.
pub(crate) fn artifact_client(source: &Base, url: &str) -> Result<Client, synra_ipc::IpcError> {
    let parsed = NetworkGuard::for_hosts(&[])
        .allowing_insecure(!source.secure())
        .parse(url)
        .map_err(|error| {
            synra_ipc::IpcError::unknown(error.code)
                .with_params(serde_json::json!({"reason": error.message}))
        })?;
    let allow_http = !source.secure() && synra_registry::is_loopback_host(&parsed.host);
    Ok(guarded_client(
        NetworkGuard::for_hosts(&[parsed.host])
            .allowing_insecure(allow_http)
            .with_policy(IpPolicy {
                allow_loopback: source.loopback(),
                ..IpPolicy::default()
            }),
    ))
}

fn guarded_client(guard: NetworkGuard) -> Client {
    Client::new(
        guard,
        FetchLimits {
            max_response_bytes: MAX_DOWNLOAD_BYTES,
            ..FetchLimits::default()
        },
    )
}
