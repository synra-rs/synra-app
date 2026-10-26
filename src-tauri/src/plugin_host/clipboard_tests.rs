//! The desktop host must preserve clipboard text across its gated plugin boundary.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use synra_capability::CapabilitySet;
use synra_platform::{CommandPlatform, CommandRunner, Desktop, PlatformError};
use synra_plugin_host::{
    host_calls::{CallError, HostCalls},
    quota::{Budget, Quota},
};
use synra_protocol::PluginId;

use super::DesktopHost;

struct ClipboardRunner {
    text: String,
    calls: Arc<AtomicUsize>,
    program: &'static str,
    arguments: Vec<String>,
}

impl CommandRunner for ClipboardRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<String, PlatformError> {
        assert_eq!(program, self.program);
        assert_eq!(arguments, self.arguments);
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.text.clone())
    }
}

#[test]
fn desktop_clipboard_text_survives_gated_host_calls_without_loss_or_json_confusion() {
    for text in ["", "null", "\"quoted\"\r\n中文🖱️\\"] {
        let source = super::tests::host_with(&[("platform:read-clipboard", true)]);
        let count = Arc::new(AtomicUsize::new(0));
        let host = DesktopHost::with_platform(
            Arc::clone(&source.db),
            Arc::new(CommandPlatform::with(
                Desktop::MacOs,
                Box::new(ClipboardRunner {
                    text: text.to_owned(),
                    calls: Arc::clone(&count),
                    program: "pbpaste",
                    arguments: Vec::new(),
                }),
            )),
        );
        let budget = Budget::new(Quota::default());
        let mut audit = Vec::new();
        let mut grants = CapabilitySet::new();
        grants
            .insert_str("platform:read-clipboard")
            .expect("valid grant");
        let mut calls = HostCalls::new(
            &host,
            PluginId::parse("@acme/notes").expect("id"),
            &grants,
            &budget,
            &mut audit,
        );
        let result = calls
            .call("synra:plugin/host#platform-read-clipboard", "")
            .expect("clipboard text");
        assert_eq!(result, serde_json::to_string(text).expect("JSON string"));
        assert_eq!(
            serde_json::from_str::<Option<String>>(&result).expect("nullable text"),
            Some(text.to_owned())
        );
        assert_eq!(count.load(Ordering::Relaxed), 1);

        let no_grants = CapabilitySet::new();
        let mut denied = HostCalls::new(
            &host,
            PluginId::parse("@acme/notes").expect("id"),
            &no_grants,
            &budget,
            &mut audit,
        );
        let error = denied
            .call("synra:plugin/host#platform-read-clipboard", "")
            .expect_err("missing grant");
        assert_eq!(error.code, CallError::CAPABILITY_DENIED);
        assert_eq!(
            count.load(Ordering::Relaxed),
            1,
            "a denied call did not run a command"
        );
    }
}

#[test]
fn a_gated_open_url_call_reaches_the_desktop_backend() {
    let source = super::tests::host_with(&[("platform:open-url", true)]);
    let count = Arc::new(AtomicUsize::new(0));
    let url = "https://example.com/path?q=one&other=two";
    let host = DesktopHost::with_platform(
        Arc::clone(&source.db),
        Arc::new(CommandPlatform::with(
            Desktop::MacOs,
            Box::new(ClipboardRunner {
                text: String::new(),
                calls: Arc::clone(&count),
                program: "open",
                arguments: vec![url.to_owned()],
            }),
        )),
    );
    let budget = Budget::new(Quota::default());
    let mut audit = Vec::new();
    let mut grants = CapabilitySet::new();
    grants.insert_str("platform:open-url").expect("grant");
    let mut calls = HostCalls::new(
        &host,
        PluginId::parse("@acme/notes").expect("id"),
        &grants,
        &budget,
        &mut audit,
    );
    assert_eq!(
        calls
            .call("synra:plugin/host#platform-open-url", url)
            .expect("opened"),
        "null"
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    let no_grants = CapabilitySet::new();
    let mut denied = HostCalls::new(
        &host,
        PluginId::parse("@acme/notes").expect("id"),
        &no_grants,
        &budget,
        &mut audit,
    );
    assert_eq!(
        denied
            .call("synra:plugin/host#platform-open-url", url)
            .expect_err("denied")
            .code,
        CallError::CAPABILITY_DENIED
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);
}
