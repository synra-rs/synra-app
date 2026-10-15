//! What a plugin's host calls reach, in the desktop shell.
//!
//! # Why this module had to exist
//!
//! Because the shell's plugin set was built with **no host at all**. `PluginSet::new` took one argument when this file
//! was written and two when the capability gate was finally connected to anything — so `main.rs` stopped compiling and
//! nothing said so, because the check being run was `cargo test --lib` and this code lives in the binary.
//!
//! The signature was not an accident: an instance with no session refuses every host call, which is what made the gate
//! inert for months. Requiring the host made that state unrepresentable, and the cost was that a shell which had never
//! supplied one had to say what its host was.
//!
//! # What it serves, and what it refuses
//!
//! The same rule as the daemon's: answer where this build genuinely knows, refuse with a code where it does not. The
//! difference is the point of having two — a shell has a desktop, so `platform:*` reaches
//! [`CommandPlatform`](synra_platform::CommandPlatform), which opens a browser, reads the clipboard and shows a
//! notification for real. The daemon is headless and refuses the same calls; `synra-platform` had the desktop
//! implementation, tested, and **no caller** until this file.

use std::sync::Arc;

use parking_lot::Mutex;

use synra_capability::CapabilitySet;
use synra_plugin_host::host_calls::{CallError, Host};
use synra_protocol::ids::PluginId;
use synra_store::SynraDb;

/// The desktop shell's answer to a plugin's host calls.
pub struct DesktopHost {
    /// The database, for the questions answered from what is recorded: which capabilities the user granted, and the
    /// devices this machine knows.
    db: Arc<Mutex<SynraDb>>,
    /// The desktop, over `synra-platform`'s command runner.
    ///
    /// ★ The piece that was implemented, tested and unreachable. It opens URLs, reveals paths, reads and writes the
    /// clipboard, shows notifications and simulates a key chord — each of them behind the guard, so a malformed URL is
    /// refused before anything is launched.
    platform: synra_platform::CommandPlatform,
}

impl DesktopHost {
    /// A host over the shell's own state.
    #[must_use]
    pub fn new(db: Arc<Mutex<SynraDb>>) -> Self {
        Self { db, platform: synra_platform::CommandPlatform::default() }
    }

    /// The capabilities the user granted this plugin, as the database records them.
    fn granted(&self, plugin: &PluginId) -> Vec<String> {
        self.db
            .lock()
            .plugin_grants(plugin.as_str())
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.granted)
            .map(|row| row.capability)
            .collect()
    }
}

/// The refusal for everything this shell does not implement.
fn not_implemented(group: &str) -> CallError {
    CallError::new(
        CallError::NOT_IMPLEMENTED,
        format!("this shell does not implement {group}"),
    )
}

/// Turns a platform refusal into a code a plugin can branch on.
///
/// # Why the two are distinguished
///
/// Because they send a plugin author to different places. `badArgument` means the value it supplied is the problem — a
/// `javascript:` URL, a path outside the sandbox, a chord that is not a chord — and the author can fix it. `notImplemented`
/// means the *installation* has no such facility, which no amount of editing will change.
fn platform_refusal(error: &synra_platform::PlatformError) -> CallError {
    use synra_platform::PlatformError;
    let code = match error {
        PlatformError::BadUrl { .. }
        | PlatformError::BadPath { .. }
        | PlatformError::TooLong { .. }
        | PlatformError::SimulateKeysNotGranted => CallError::BAD_ARGUMENT,
        PlatformError::Unsupported(_) | PlatformError::Unavailable(_) => CallError::NOT_IMPLEMENTED,
        PlatformError::NoRecentInteraction { .. }
        | PlatformError::RateLimited { .. }
        | PlatformError::QuotaExceeded { .. }
        | PlatformError::RunnerFailed { .. } => CallError::HOST_FAILED,
        // `PlatformError` is `#[non_exhaustive]`, so a variant added later lands here rather than failing to compile.
        // `hostFailed` is the honest default: telling a plugin its argument was wrong when the fault may be the shell's
        // would be worse than a code it simply retries on.
        _ => CallError::HOST_FAILED,
    };
    CallError::new(code, error.to_string())
}

impl Host for DesktopHost {
    fn sdk_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    fn host_kind(&self) -> String {
        "desktop".to_owned()
    }

    fn locale(&self) -> String {
        // No interface setting is read here yet, and an infallible question cannot say "I do not know" — see the daemon's
        // host, which has the same problem for the same reason.
        String::new()
    }

    fn dark_mode(&self) -> bool {
        false
    }

    fn can(&self, plugin: &PluginId, capability: &str) -> bool {
        // The same matching the gate uses, so the answer a plugin gets about itself agrees with the answer it will get
        // when it tries: a string comparison would make `log:*` and `log:info` different capabilities.
        let Ok(needed) = synra_capability::Capability::parse(capability) else {
            return false;
        };
        let mut held = CapabilitySet::new();
        for text in self.granted(plugin) {
            let _ = held.insert_str(&text);
        }
        held.grants(&needed)
    }

    fn platform_capabilities(&self) -> Vec<String> {
        // What this desktop can actually do rather than what a build might — `synra-platform` answers for the machine it
        // is running on, and inventing a list here would be a promise this shell cannot keep.
        synra_platform::Platform::supported_actions(&self.platform)
            .iter()
            .map(|action| action.capability().to_owned())
            .collect()
    }

    fn log(
        &self,
        plugin: &PluginId,
        level: &str,
        message: &str,
        context: Option<&str>,
    ) -> Result<(), CallError> {
        // To stderr, which is where a shell's own diagnostics go. The daemon routes this through `tracing` because it
        // has a log file; a windowed shell has a terminal when it was started from one and nothing when it was not.
        eprintln!("synra: [{level}] {} {}{message}", plugin.as_str(), context.map_or(String::new(), |c| format!("({c}) ")));
        Ok(())
    }

    fn devices(&self) -> Vec<(String, String, String)> {
        // What this side can actually reach, not everything it has ever paired with: an unreachable device is one a send
        // would fail against, and offering it turns a list into a promise.
        let rows = self.db.lock().paired_devices().unwrap_or_default();
        rows.iter()
            .filter(|device| device.is_usable())
            .map(|device| {
                (device.device_id.clone(), device.display_name.clone(), device.platform.clone())
            })
            .collect()
    }

    fn platform(
        &self,
        _plugin: &PluginId,
        action: &str,
        argument: &str,
    ) -> Result<Option<String>, CallError> {
        use synra_platform::Platform as _;
        // A notification carries a title and a body separated by the contract's own delimiter, which is the shape
        // `host_calls` builds when it sends one.
        let outcome = match action {
            "open-url" => self.platform.open_url(argument),
            "open-path" => self.platform.open_path(std::path::Path::new(argument)),
            "reveal-path" => self.platform.reveal_path(std::path::Path::new(argument), None),
            "simulate-keys" => self.platform.simulate_keys(argument),
            "read-clipboard" => self.platform.read_clipboard().map(|_| ()),
            "write-clipboard" => self.platform.write_clipboard(argument),
            "notify" => {
                let (title, body) = argument.split_once('\u{1}').unwrap_or((argument, ""));
                self.platform.notify(title, body)
            }
            other => {
                return Err(CallError::new(
                    CallError::BAD_ARGUMENT,
                    format!("`{other}` is not a platform action"),
                ));
            }
        };
        outcome.map_or_else(|error| Err(platform_refusal(&error)), |()| Ok(None))
    }

    // ── Everything below is refused, and says which group it belongs to ────────────────────────────────────────────
    //
    // The rule is the daemon host's rule: a function is answered where this shell genuinely knows, and refused with a
    // code where it does not. Refusing is not a placeholder — a plugin branches on the code, and `notImplemented` is a
    // fact it can act on.

    fn event_emit(&self, _p: &PluginId, _e: &str, _payload: &str) -> Result<(), CallError> {
        Err(not_implemented("event emission"))
    }

    fn device_send(&self, _p: &PluginId, _t: &str, _e: &str, _pl: &str) -> Result<(), CallError> {
        Err(not_implemented("device sends"))
    }

    fn device_broadcast(&self, _p: &PluginId, _e: &str, _pl: &str) -> Result<(), CallError> {
        Err(not_implemented("device broadcasts"))
    }

    fn storage_get(&self, _p: &PluginId, _k: &str) -> Result<Option<String>, CallError> {
        Err(not_implemented("plugin storage"))
    }

    fn storage_set(&self, _p: &PluginId, _k: &str, _v: &str) -> Result<(), CallError> {
        Err(not_implemented("plugin storage"))
    }

    fn storage_delete(&self, _p: &PluginId, _k: &str) -> Result<(), CallError> {
        Err(not_implemented("plugin storage"))
    }

    fn storage_keys(&self, _p: &PluginId, _prefix: Option<&str>) -> Result<Vec<String>, CallError> {
        Err(not_implemented("plugin storage"))
    }

    fn storage_quota(&self, _p: &PluginId) -> Result<(u64, u64), CallError> {
        Err(not_implemented("plugin storage"))
    }

    fn secret_get(&self, _p: &PluginId, _k: &str) -> Result<Option<String>, CallError> {
        Err(not_implemented("secrets"))
    }

    fn secret_set(&self, _p: &PluginId, _k: &str, _v: &str) -> Result<(), CallError> {
        Err(not_implemented("secrets"))
    }

    fn secret_delete(&self, _p: &PluginId, _k: &str) -> Result<(), CallError> {
        Err(not_implemented("secrets"))
    }

    fn fetch(&self, _p: &PluginId, _request_json: &str) -> Result<String, CallError> {
        // No HTTP client exists anywhere in this workspace, which is a fact rather than an omission here.
        Err(not_implemented("network access"))
    }

    fn transfer_send(&self, _p: &PluginId, _k: &str, _t: &str, _m: &str) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn transfer_status(&self, _p: &PluginId, _id: &str) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn action_declare(&self, _p: &PluginId, _n: &str, _s: &str) -> Result<(), CallError> {
        Err(not_implemented("actions"))
    }

    fn action_list(&self, _p: &PluginId) -> Result<String, CallError> {
        Err(not_implemented("actions"))
    }

    fn action_invoke(&self, _p: &PluginId, _t: &str, _n: &str, _a: &str) -> Result<String, CallError> {
        Err(not_implemented("actions"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use synra_plugin_host::host_calls::Host as _;
    use synra_protocol::ids::PluginId;

    use super::DesktopHost;

    fn id(name: &str) -> PluginId {
        PluginId::try_from(name.to_owned()).expect("a plugin id")
    }

    fn host_with(grants: &[(&str, bool)]) -> DesktopHost {
        let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
        db.migrate().expect("migrates");
        db.insert_plugin("@acme/notes", true, true, "healthy", 1, 1)
            .expect("a plugin row");
        let decisions: Vec<(String, bool)> =
            grants.iter().map(|(cap, granted)| ((*cap).to_owned(), *granted)).collect();
        db.record_grants("@acme/notes", &decisions, 1, "digest").expect("decisions");
        DesktopHost::new(Arc::new(Mutex::new(db)))
    }

    #[test]
    fn the_shell_is_a_desktop_rather_than_a_headless_build() {
        // ★ The whole reason this host exists rather than the daemon's.
        //
        // A shell has a desktop, so `host_kind` says so and `platform:*` reaches the real implementation instead of
        // refusing. The daemon answers `headless` to the same question, and a plugin that reads it can tell which machine
        // it is on.
        let host = host_with(&[]);

        assert_eq!(host.host_kind(), "desktop");
        // ★ And it names what this machine can do.
        //
        // The daemon answers with an empty list, because it can do none of it. This one asks `synra-platform` for the
        // actions the running system supports, which is the difference between "no desktop" and "a desktop that cannot
        // simulate keys".
        assert!(
            !host.platform_capabilities().is_empty(),
            "a desktop supports something"
        );
        assert!(
            host.platform_capabilities()
                .iter()
                .any(|name| name.starts_with("platform:")),
            "{:?}",
            host.platform_capabilities()
        );
    }

    #[test]
    fn a_bad_address_is_the_plugins_mistake_rather_than_a_missing_feature() {
        // ★ The guard runs before anything is launched, and the code says whose problem it is.
        //
        // `javascript:` is a value the plugin supplied, so the refusal is `badArgument` — an author can fix that. A
        // refusal shaped like "this shell cannot open a browser" would send them looking at the installation instead.
        let host = host_with(&[]);
        let plugin = id("@acme/notes");

        let refused = host
            .platform(&plugin, "open-url", "javascript:alert(1)")
            .expect_err("the guard refuses this scheme");
        assert_eq!(refused.code, "badArgument", "{refused:?}");

        // And a name that is not a platform action is the caller's mistake rather than either of those.
        let refused = host
            .platform(&plugin, "not-an-action", "")
            .expect_err("that is not an action");
        assert_eq!(refused.code, "badArgument", "{refused:?}");
    }

    #[test]
    fn can_answers_from_what_the_user_actually_granted() {
        // The same reading the daemon makes, for the same reason: the answer a plugin gets about itself has to agree
        // with the gate that will judge it.
        let host = host_with(&[("log:*", true), ("device:list", false)]);

        assert!(host.can(&id("@acme/notes"), "log:info"));
        assert!(!host.can(&id("@acme/notes"), "device:list"));
        assert!(!host.can(&id("@acme/other"), "log:info"));
        assert!(!host.can(&id("@acme/notes"), "not a capability"));
    }

    #[test]
    fn the_groups_this_shell_does_not_implement_refuse_with_one_code() {
        // A plugin branches on the code, and `notImplemented` says "this shell cannot do that" rather than "you are not
        // allowed" — which is the difference between hiding a feature and hunting for a permission already granted.
        let host = host_with(&[]);
        let plugin = id("@acme/notes");

        for refusal in [
            host.event_emit(&plugin, "x", "{}"),
            host.device_send(&plugin, "d", "x", "{}"),
            host.storage_get(&plugin, "k").map(|_| ()),
            host.fetch(&plugin, "{}").map(|_| ()),
            host.action_list(&plugin).map(|_| ()),
        ] {
            let error = refusal.expect_err("this shell does not implement it");
            assert_eq!(error.code, "notImplemented", "{error:?}");
        }
    }
}
