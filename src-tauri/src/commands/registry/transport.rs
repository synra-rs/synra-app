use super::*;

/// The configured registries, in the order they are tried.
///
/// The key holds either one URL or an array of mirrors; an array is tried in order, and only for failures that another
/// mirror could plausibly fix (16 §4.2). A verification failure never falls back — it is not a network problem, and
/// retrying it somewhere else would be skipping the check the design exists to keep.
pub(crate) fn bases(state: &AppState) -> Result<Vec<Base>, synra_ipc::IpcError> {
    let rows = state.db().config().map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let Some(row) = rows.iter().find(|row| row.key == REGISTRY_KEY) else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": format!("no registry is configured; set `{REGISTRY_KEY}` to its base URL first"),
        })));
    };
    let value: serde_json::Value = serde_json::from_str(&row.value).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": REGISTRY_KEY, "reason": error.to_string()}))
    })?;
    let urls: Vec<String> = match value {
        serde_json::Value::String(url) => vec![url],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => {
            return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"field": REGISTRY_KEY, "reason": "it must be a URL or an array of URLs"}),
            ));
        }
    };
    let parsed: Vec<Base> = urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .map(|url| match url.strip_prefix("static:") {
            Some(rest) => split_url(rest.trim(), Kind::Static),
            None => split_url(url, Kind::Npm),
        })
        .collect::<Result<_, _>>()?;
    if parsed.is_empty() {
        return Err(
            synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "why": format!("`{REGISTRY_KEY}` names no registry"),
            })),
        );
    }
    Ok(parsed)
}

/// Splits a base URL into its scheme, authority and host.
pub(crate) fn split_url(url: &str, kind: Kind) -> Result<Base, synra_ipc::IpcError> {
    let refused = |reason: &str| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": REGISTRY_KEY, "reason": reason}))
    };
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(refused("it must start with https:// or http://"));
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(refused("it names no host"));
    }
    let path = rest[authority.len()..].trim_end_matches('/');
    // An npm registry's paths are the protocol's, so a path in its URL would be silently ignored; a static host's prefix
    // is where the bucket's files live, so a path is the point there.
    if kind == Kind::Npm && !path.is_empty() {
        return Err(refused(
            "an npm-compatible registry names only a scheme and a host",
        ));
    }
    // A port is allowed.
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    };
    let host = host.trim_matches(['[', ']']).to_owned();
    if host.is_empty() {
        return Err(refused("it names no host"));
    }
    let scheme = if secure { "https" } else { "http" };
    let authority_with_port = match port {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    let origin = format!("{scheme}://{authority_with_port}{path}");
    Ok(Base {
        origin,
        host,
        secure,
        kind,
    })
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
    what: &str,
) -> Result<(Base, synra_fetch::Response), synra_ipc::IpcError> {
    let mut last: Option<(ErrorCode, String)> = None;
    for base in bases {
        let path = path_for(base);
        let url = format!("{}{path}", base.origin);
        match client(base).fetch(&get(&url)) {
            Ok(response) if (200..300).contains(&response.status) => {
                return Ok((base.clone(), response));
            }
            Ok(response) if retryable(response.status) => {
                last = Some((
                    ErrorCode::Unsupported,
                    format!("{url} answered {}", response.status),
                ));
            }
            Ok(response) => {
                return Err(
                    synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                        serde_json::json!({
                            "url": url,
                            "status": response.status,
                            "reason": format!("{what} refused the request"),
                        }),
                    ),
                );
            }
            Err(refusal) => {
                last = Some((refusal.code, format!("{url}: {}", refusal.message)));
            }
        }
    }
    let (code, reason) =
        last.unwrap_or((ErrorCode::Unsupported, format!("no registry serves {what}")));
    Err(
        synra_ipc::IpcError::unknown(code).with_params(serde_json::json!({
            "reason": reason,
            "tried": bases.len(),
        })),
    )
}

/// The guarded HTTP client for one registry.
pub(crate) fn client(base: &Base) -> Client {
    let mut guard = NetworkGuard::for_hosts(std::slice::from_ref(&base.host));
    if !base.secure {
        // Plain http is only for a loopback registry — a local mirror, a test server. A public host must be https, which
        // is the guard's default rather than a rule added here.
        guard = guard.allowing_insecure(true).with_policy(IpPolicy {
            allow_loopback: true,
            ..IpPolicy::default()
        });
    }
    Client::new(
        guard,
        FetchLimits {
            max_response_bytes: MAX_DOWNLOAD_BYTES,
            ..FetchLimits::default()
        },
    )
}
