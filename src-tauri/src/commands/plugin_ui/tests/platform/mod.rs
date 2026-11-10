use super::*;

mod failures;

// ── The desktop: clipboard, browser, notifications ──────────────────────────────────────────────

/// Every program the fake runner was asked to launch, with its arguments.
type Calls = Arc<Mutex<Vec<(String, Vec<String>, Option<String>)>>>;

/// A platform whose programs are recorded instead of launched.
///
/// ★ Why the tests need one: every platform action ends in a program on this machine — `powershell` for the clipboard on
/// Windows, `pbcopy` on macOS, `notify-send` on Linux — and a test that really ran them would open a browser, overwrite
/// the user's clipboard or show a notification. The production code is unchanged; only the runner under it is.
#[derive(Clone)]
struct RecordingRunner {
    calls: Calls,
    answer: String,
}

impl CommandRunner for RecordingRunner {
    fn is_available(&self) -> bool {
        true
    }
    fn run(
        &self,
        program: &str,
        arguments: &[&str],
        input: Option<&str>,
    ) -> Result<String, PlatformError> {
        self.calls.lock().push((
            program.to_owned(),
            arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            input.map(str::to_owned),
        ));
        Ok(self.answer.clone())
    }
}

/// A state whose platform records instead of acting, and a page holding these grants.
fn platform_page(
    grants: &[&str],
) -> (
    crate::testing::Root,
    crate::AppState,
    String,
    RecordingRunner,
) {
    let (root, mut state) = crate::testing::file_state();
    let answer = if cfg!(windows) {
        serde_json::to_string("from the clipboard").expect("nullable text JSON")
    } else {
        "from the clipboard".to_owned()
    };
    let runner = RecordingRunner {
        calls: Arc::new(Mutex::new(Vec::new())),
        answer,
    };
    state.platform = Arc::new(CommandPlatform::with(
        Desktop::current(),
        Box::new(runner.clone()),
    ));
    let token = hosted_page_with(&state, grants);
    (root, state, token, runner)
}

#[test]
fn reading_the_clipboard_answers_with_the_platforms_text() {
    let (_root, state, token, runner) = platform_page(&["platform:read-clipboard"]);

    let answer = super::super::plugin_ui_rpc(&state, &token, "clipboard.read", &serde_json::json!({}))
        .expect("the clipboard");

    assert_eq!(answer, serde_json::json!("from the clipboard"));
    assert_eq!(
        runner.calls.lock().len(),
        1,
        "the platform was asked exactly once"
    );
}

#[test]
fn writing_the_clipboard_without_the_write_grant_never_reaches_the_platform() {
    // ★ The gate, not the façade: the `denied` a page sees from `synra.clipboard.write` is an optimisation; the check
    // that matters is the one between a call and the user's clipboard.
    let (_root, state, token, runner) = platform_page(&["platform:read-clipboard"]);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": "x"}),
    )
    .expect_err("no write capability");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused
            .params
            .get("capability")
            .and_then(|value| value.as_str()),
        Some("platform:write-clipboard")
    );
    assert!(
        runner.calls.lock().is_empty(),
        "a refused write launched a program"
    );
}

#[test]
fn clipboard_write_delivers_text_to_the_process_stdin() {
    let (_root, state, token, runner) = platform_page(&["platform:write-clipboard"]);
    let text = "\"literal\"; $(1+2)\r\n中文🖱️";
    let answer = super::super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": text}),
    )
    .expect("clipboard write");
    assert_eq!(answer, serde_json::Value::Null);
    assert_eq!(runner.calls.lock().len(), 1);
    let call = runner.calls.lock().first().cloned().expect("one command");
    assert_eq!(call.2.as_deref(), Some(text));
    assert!(!call.1.join(" ").contains(text));
}

#[test]
fn oversized_clipboard_utf8_is_refused_before_a_process_starts() {
    let (_root, state, token, runner) = platform_page(&["platform:write-clipboard"]);
    let text = "选".repeat(synra_platform::MAX_CLIPBOARD_BYTES / 3 + 1);
    let error = super::super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": text}),
    )
    .expect_err("byte limit");
    assert_eq!(error.code, ErrorCode::PayloadTooLarge);
    assert!(runner.calls.lock().is_empty());
}

#[test]
fn opening_a_url_hands_it_to_the_platform() {
    let (_root, state, token, runner) = platform_page(&["platform:open-url"]);

    let answer = super::super::plugin_ui_rpc(
        &state,
        &token,
        "platform.openUrl",
        &serde_json::json!({"url": "https://example.com/page"}),
    )
    .expect("a URL");

    assert_eq!(answer, serde_json::Value::Null);
    // The guard is scoped: `calls` protects a `Vec` that nothing else in this test needs after the assertion, and
    // holding it across the rest of the test is what `significant_drop_tightening` refuses.
    let reached_url = {
        let calls = runner.calls.lock();
        assert_eq!(calls.len(), 1, "the opener was launched once");
        calls[0]
            .1
            .iter()
            .any(|argument| argument == "https://example.com/page")
    };
    assert!(reached_url, "the URL did not reach the platform");
}

#[test]
fn a_url_the_platform_refuses_never_reaches_the_runner() {
    let (_root, state, token, runner) = platform_page(&["platform:open-url"]);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "platform.openUrl",
        &serde_json::json!({"url": "file:///etc/passwd"}),
    )
    .expect_err("a local scheme is not opened");

    // The refusal is about the argument, and it names the field — a user can act on that; "internal" would send them to
    // the logs for a URL the policy already explained.
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("url")
    );
    assert!(
        runner.calls.lock().is_empty(),
        "a refused URL was opened anyway"
    );
}

#[test]
fn a_notification_carries_its_title_and_body() {
    let (_root, state, token, runner) = platform_page(&["platform:notify"]);

    super::super::plugin_ui_rpc(
        &state,
        &token,
        "platform.notify",
        &serde_json::json!({"title": "Build", "body": "it finished"}),
    )
    .expect("a notification");

    let joined = {
        let calls = runner.calls.lock();
        assert_eq!(calls.len(), 1, "the notifier was launched once");
        if cfg!(windows) {
            calls[0].2.clone().expect("notification stdin")
        } else {
            calls[0].1.join(" ")
        }
    };
    assert!(
        joined.contains("Build"),
        "the title did not reach the notifier: {joined}"
    );
    assert!(
        joined.contains("it finished"),
        "the body did not reach the notifier: {joined}"
    );
}
