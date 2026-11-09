use super::*;

pub(super) fn record_pairing(
    state: &crate::state::AppState,
    dialled: &synra_core::host::Dialled,
    address: std::net::SocketAddr,
) {
    use synra_protocol::session::PlatformKind;

    let negotiated = dialled.negotiated();
    let platform = match negotiated.platform {
        PlatformKind::Desktop => "desktop",
        PlatformKind::Android => "android",
        PlatformKind::Ios => "ios",
        _ => "unknown",
    };
    let record = synra_store::Paired {
        device_id: negotiated.device_id.to_string(),
        display_name: negotiated.display_name.clone(),
        platform: platform.to_owned(),
        features: serde_json::to_string(
            &negotiated
                .agreed
                .common
                .iter()
                .map(|feature| feature.as_str())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_owned()),
        address: Some(address.to_string()),
        paired_at_ms: i64::try_from(unix_millis()).unwrap_or(0),
    };
    let _ = state.db().pair_device(&record);
}
