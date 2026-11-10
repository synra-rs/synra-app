use super::*;

#[test]
fn a_page_uploads_a_file_in_chunks_and_the_host_keeps_it_until_finished() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let encoded = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);

    let begun = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "big.bin",
            "totalBytes": 5,
        }),
    )
    .expect("the upload begins");
    let upload = begun
        .get("uploadId")
        .and_then(|value| value.as_str())
        .expect("an upload id")
        .to_owned();

    let uploads = state.paths.cache_dir().join("plugin-transfers");
    assert_eq!(
        std::fs::read_dir(&uploads)
            .map(|entries| entries.count())
            .unwrap_or(0),
        1,
        "the upload owns one directory while it is open"
    );

    super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": encoded(b"hello")}),
    )
    .expect("the chunk is appended");

    // More bytes than the page declared is refused rather than written, and the upload stays open so the page can still
    // abort it or retry with the right bytes.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": encoded(b"!")}),
    )
    .expect_err("over the declared size");
    assert_eq!(refused.code, ErrorCode::PayloadTooLarge);

    // An upload id this host does not know is answered as a bad argument, not as a send.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": "not-an-upload", "data": encoded(b"x")}),
    )
    .expect_err("no such upload");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);

    let aborted = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.abort",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect("the upload aborts");
    assert_eq!(aborted.get("aborted"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        std::fs::read_dir(&uploads)
            .map(|entries| entries.count())
            .unwrap_or(0),
        0,
        "the upload's directory went with it"
    );
}

#[test]
fn a_chunked_transfer_finished_with_the_managed_state_answers_before_the_dial() {
    // ★ The managed entry is the one the Tauri command uses. `finish` hands the upload to a worker and answers its id; the
    // dial (to a dead address) happens afterwards, so a failure arrives as a removed row rather than as a paused command.
    let (_root, state) = crate::testing::file_state();
    let token = hosted_page_with(&state, &["file:transfer:send"]);
    let device = synra_protocol::ids::DeviceId::new();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: device.to_string(),
            display_name: "Study PC".to_owned(),
            platform: "windows".to_owned(),
            features: "[]".to_owned(),
            address: Some("127.0.0.1:1".to_owned()),
            paired_at_ms: 1_000,
        })
        .expect("a paired device");
    let state = std::sync::Arc::new(state);

    let begun = super::super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({"to": device.to_string(), "fileName": "a.bin", "totalBytes": 2}),
    )
    .expect("the upload begins");
    let upload = begun
        .get("uploadId")
        .and_then(|value| value.as_str())
        .expect("an upload id")
        .to_owned();
    super::super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": "aGk="}),
    )
    .expect("the chunk is appended");

    let finished = super::super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.finish",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect("finish answers before the dial");
    assert!(
        finished.get("transferId").is_some(),
        "an id, not a completed send"
    );

    // The worker fails against port 1 and removes its row; the test waits for that rather than for a fixed sleep.
    //
    // ★ Ten seconds rather than two. The condition is right, but the budget assumed an idle machine: with the rest of the
    // suite running in parallel the worker can take longer than two seconds to be scheduled and to see the refused dial,
    // and the test then failed with "the failed send left no row" on a run where nothing was wrong. A condition-poll with
    // a generous ceiling is still a condition-poll; the assertion below is what decides, not the clock.
    for _ in 0..1_000 {
        if state.transfers.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(state.transfers.is_empty(), "the failed send left no row");
    assert!(
        !state
            .paths
            .cache_dir()
            .join("plugin-transfers")
            .read_dir()
            .map(|entries| entries.count() > 0)
            .unwrap_or(false),
        "the upload's files were left in the cache"
    );
}

#[test]
fn a_chunked_transfer_the_page_never_finishes_uses_no_send() {
    // ★ Beginning and aborting is the "page went away" path: the host must not dial anything, and the only thing left is
    // that the upload is gone.
    let (_root, state, token) = transfer_page();
    let begun = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "a.bin",
            "totalBytes": 4,
        }),
    )
    .expect("the upload begins");
    let upload = begun
        .get("uploadId")
        .and_then(|value| value.as_str())
        .expect("an id");

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.finish",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect_err("the declared bytes never arrived");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(state.transfers.is_empty(), "nothing was sent");
}
