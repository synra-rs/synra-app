use super::*;

#[test]
fn command_failures_keep_limits_and_cleanup_state_in_ui_errors() {
    struct RefusingCommand(PlatformError);
    impl CommandRunner for RefusingCommand {
        fn is_available(&self) -> bool {
            true
        }
        fn run(
            &self,
            _program: &str,
            _arguments: &[&str],
            _input: Option<&str>,
        ) -> Result<String, PlatformError> {
            Err(self.0.clone())
        }
    }
    for (error, code, kind) in [
        (
            PlatformError::RunnerBusy { limit: 8 },
            ErrorCode::RateLimited,
            "busy",
        ),
        (
            PlatformError::RunnerTimedOut {
                program: "fixture".to_owned(),
                timeout_ms: 20,
                cleanup_pending: true,
            },
            ErrorCode::Timeout,
            "timeout",
        ),
        (
            PlatformError::RunnerOutputTooLong {
                program: "fixture".to_owned(),
                stream: "stdout",
                max: 64,
            },
            ErrorCode::PayloadTooLarge,
            "outputLimit",
        ),
        (
            PlatformError::RunnerCleanupFailed {
                program: "fixture".to_owned(),
            },
            ErrorCode::Internal,
            "cleanupPending",
        ),
        (
            PlatformError::RunnerFailed {
                program: "fixture".to_owned(),
                reason: "sensitive-clipboard-content".to_owned(),
            },
            ErrorCode::Internal,
            "commandFailed",
        ),
    ] {
        let (_root, mut state, token, _) = platform_page(&["platform:read-clipboard"]);
        state.platform = Arc::new(CommandPlatform::with(
            Desktop::MacOs,
            Box::new(RefusingCommand(error)),
        ));
        let error = super::super::super::plugin_ui_rpc(&state, &token, "clipboard.read", &serde_json::json!({}))
            .expect_err("command refusal");
        assert_eq!(error.code, code);
        assert_eq!(error.params["kind"], kind);
        assert!(
            !error
                .params
                .to_string()
                .contains("sensitive-clipboard-content")
        );
        if kind == "timeout" {
            assert_eq!(error.params["timeoutMs"], 20);
            assert_eq!(error.params["cleanupPending"], true);
        }
    }
}
