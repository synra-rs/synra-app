use super::*;

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
    label: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    use tauri::Manager as _;

    fn answer(status: u16, body: &'static str) -> tauri::http::Response<Cow<'static, [u8]>> {
        tauri::http::Response::builder()
            .status(status)
            .header(
                tauri::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )
            .body(Cow::Borrowed(body.as_bytes()))
            .unwrap_or_else(|_| {
                tauri::http::Response::new(Cow::Borrowed(b"the response could not be built"))
            })
    }

    let state = app.state::<std::sync::Arc<AppState>>();
    let uri = request.uri().to_string();

    match page_resource(&state, label, &uri) {
        Ok((bytes, content_type)) => document_response(bytes, content_type),
        Err(refusal) => {
            // 403 for a path the rules refuse, 404 for a file that is not there: the first is somebody asking for something
            // they may never have, the second is a bundle that is missing a file it should carry.
            let status = if matches!(
                refusal.code,
                ErrorCode::InvalidArgument | ErrorCode::CapabilityDenied
            ) {
                403
            } else {
                404
            };
            answer(status, "no page by that name")
        }
    }
}
