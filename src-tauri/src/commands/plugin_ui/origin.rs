//! URL-safe per-package origins and the exact file document opened by a plugin window.

use sha2::{Digest as _, Sha256};
use synra_ipc::IpcError;
use synra_protocol::{ErrorCode, PluginId};

const SCHEME: &str = "synra-plugin";

pub(super) fn host(id: &PluginId) -> String {
    let digest = Sha256::digest(id.as_str().as_bytes());
    let first: String = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let second: String = digest
        .iter()
        .skip(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("p{first}.{second}.localhost")
}

pub(super) fn label(id: &PluginId) -> String {
    format!("plugin-{}", host(id).replace('.', "-"))
}

pub(super) fn entry(id: &PluginId, path: &str, page: Option<&str>) -> Result<String, IpcError> {
    let invalid = |reason: &str| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason":reason}))
    };
    synra_plugin_ui::artifact::check_path(path).map_err(|error| invalid(&error.to_string()))?;
    let mut url = url::Url::parse(&format!("{SCHEME}://{}/", host(id)))
        .map_err(|_| invalid("the plugin origin could not be constructed"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| invalid("the plugin origin has no path"))?;
        segments.clear();
        for segment in path.split('/') {
            segments.push(segment);
        }
    }
    if let Some(page) = page {
        synra_plugin_ui::artifact::check_path(page).map_err(|error| invalid(&error.to_string()))?;
        url.set_fragment(Some(page));
    }
    Ok(url.to_string())
}

/// Normalize only the native origin and the HTTPS mapping explicitly selected by this shell.
fn document(input: &str) -> Option<(String, String, Option<String>)> {
    if input.chars().any(char::is_control) {
        return None;
    }
    // Refuse traversal before URL parsing can normalize dot segments away.
    let raw_path = input
        .split_once("://")?
        .1
        .split_once('/')?
        .1
        .split(['?', '#'])
        .next()?;
    let path = percent_encoding::percent_decode_str(raw_path)
        .decode_utf8()
        .ok()?
        .into_owned();
    synra_plugin_ui::artifact::check_path(&path).ok()?;
    let url = url::Url::parse(input).ok()?;
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return None;
    }
    let authority = match url.scheme() {
        SCHEME => url.host_str()?,
        "https" if cfg!(any(windows, target_os = "android")) => {
            url.host_str()?.strip_prefix("synra-plugin.")?
        }
        _ => return None,
    };
    Some((authority.to_owned(), path, url.query().map(str::to_owned)))
}

pub(super) fn stays(candidate: &str, opened: &str) -> bool {
    match (document(candidate), document(opened)) {
        (Some(candidate), Some(opened)) => candidate == opened,
        _ => false,
    }
}

pub(super) fn split<'a>(input: &str, known: &'a [PluginId]) -> Option<(&'a PluginId, String)> {
    let (authority, path, _) = document(input)?;
    let id = known.iter().find(|id| host(id) == authority)?;
    Some((id, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_ids_produce_distinct_valid_origins_and_window_labels() {
        let a = PluginId::parse("@a/same").unwrap();
        let b = PluginId::parse("@b/same").unwrap();
        let uri = entry(&a, "dist/index.html", Some("settings")).unwrap();
        assert_eq!(url::Url::parse(&uri).unwrap().as_str(), uri);
        assert_ne!(host(&a), host(&b));
        assert!(
            label(&a)
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        );
        let parsed = url::Url::parse(&uri).unwrap();
        let asset = parsed.join("./assets/app.js").unwrap();
        assert_eq!(
            split(asset.as_str(), std::slice::from_ref(&a)),
            Some((&a, "dist/assets/app.js".to_owned()))
        );
    }

    #[test]
    fn encoded_names_round_trip_without_double_decoding() {
        let id = PluginId::parse("@a/ui").unwrap();
        let uri = entry(&id, "中文/100% #.html", None).unwrap();
        assert_eq!(
            split(&uri, std::slice::from_ref(&id)),
            Some((&id, "中文/100% #.html".to_owned()))
        );
    }

    #[test]
    fn only_fragment_changes_preserve_the_opened_document() {
        let a = PluginId::parse("@a/ui").unwrap();
        let b = PluginId::parse("@b/ui").unwrap();
        let uri = entry(&a, "index.html", None).unwrap();
        assert!(stays(&format!("{uri}#settings"), &uri));
        for other in [
            entry(&b, "index.html", None).unwrap(),
            entry(&a, "other.html", None).unwrap(),
            format!("{uri}?changed=1"),
            uri.replace(SCHEME, "http"),
        ] {
            assert!(!stays(&other, &uri), "{other}");
        }
        let mapped = uri.replacen("synra-plugin://", "https://synra-plugin.", 1);
        assert_eq!(
            stays(&mapped, &uri),
            cfg!(any(windows, target_os = "android"))
        );
        assert!(split(&format!("{uri}/../secret"), &[a]).is_none());
    }
}
