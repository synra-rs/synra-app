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
use synra_platform::Platform as _;
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
    /// ★ Shared with `AppState` and not owned here. The platform keeps per-instance state — the notification limiter is
    /// the one that matters — and a host with its own copy would let a plugin page's `platform.notify` use an allowance
    /// the headless half had already spent, or the other way round.
    platform: Arc<synra_platform::CommandPlatform>,
    /// One fetcher per plugin, and the hosts it was built for.
    ///
    /// ★ Why a cache rather than one client per call: a client holds the plugin's **in-flight count**, so a client built per
    /// call would start from zero every time and `max_concurrent` would never refuse anything. The host list is stored
    /// beside it because the guard is built from the plugin's grants — and a user who withdraws a host must not keep
    /// reaching it through a cached client, so a changed list rebuilds the client.
    fetch_clients: Mutex<FetchClients>,
}

/// One fetcher per plugin, and the host list it was built for.
///
/// A `type` rather than the written-out type because clippy was right to complain: this appeared inline in the struct above
/// and again wherever it is read, and a reader has to parse four levels of nesting to learn that it is a map of plugins to
/// *(hosts, client)*.
type FetchClients = std::collections::HashMap<PluginId, (Vec<String>, Arc<synra_fetch::Client>)>;

impl DesktopHost {
    /// A host over the shell's own state.
    ///
    /// ★ Kept for tests and for a host that has no `AppState`; a shell uses [`DesktopHost::with_platform`] so the host and
    /// `plugin_ui_rpc` share one platform.
    #[must_use]
    pub fn new(db: Arc<Mutex<SynraDb>>) -> Self {
        Self::with_platform(db, Arc::new(synra_platform::CommandPlatform::default()))
    }

    /// A host over the state, using the platform the rest of the shell uses.
    #[must_use]
    pub fn with_platform(db: Arc<Mutex<SynraDb>>, platform: Arc<synra_platform::CommandPlatform>) -> Self {
        Self {
            db,
            platform,
            fetch_clients: Mutex::new(std::collections::HashMap::new()),
        }
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

/// The fetcher for one plugin, built or rebuilt from what the user has granted it.
///
/// # Errors
///
/// [`CallError`] with `capabilityDenied` when the plugin holds no `network:fetch:<host>` grant at all — a different answer
/// from "that host is not one of them", and the one a plugin author needs when nothing works.
fn fetcher_for(host: &DesktopHost, plugin: &PluginId) -> Result<Arc<synra_fetch::Client>, CallError> {
    let hosts = granted_hosts(host, plugin);

    // ★ An empty list is refused rather than treated as "any host".
    //
    // `network:fetch:*` is how the **gate** spells the family and a plugin can hold it, but a guard built from it would
    // have to mean "every host on the internet" — exactly the capability an SSRF guard exists to make deliberate. So a grant
    // with no host in it reaches nothing.
    if hosts.is_empty() {
        return Err(CallError::new(
            CallError::CAPABILITY_DENIED,
            format!("`{plugin}` holds no `network:fetch:<host>` grant, so there is nowhere it may fetch from"),
        ));
    }

    // ★ The lock is taken twice rather than held across the client's construction: building a client is not work that needs
    // the map, and holding it would make every plugin's fetch wait behind whichever one arrived first. Two callers arriving
    // together may each build a client and the later insert wins — which splits the in-flight count between them for as long
    // as the loser lives, a weaker limit rather than a wrong one.
    {
        let clients = host.fetch_clients.lock();
        if let Some((built_for, client)) = clients.get(plugin)
            && *built_for == hosts
        {
            return Ok(Arc::clone(client));
        }
    }

    let client = Arc::new(synra_fetch::Client::new(
        synra_core::network::NetworkGuard::for_hosts(&hosts),
        synra_fetch::Limits::default(),
    ));
    host.fetch_clients.lock().insert(plugin.clone(), (hosts, Arc::clone(&client)));
    Ok(client)
}

/// The hosts a plugin was granted for fetching.
///
/// `network:fetch:api.github.com` names a host; `network:fetch:api.github.com:POST` names a host **and a method**, and the
/// method is not this host's to enforce — the client sends whatever the plugin asked for, and the guard decides *where*. A
/// capability that is not `network:fetch:` prefixed, or whose host segment is empty or the literal `*`, contributes nothing.
fn granted_hosts(host: &DesktopHost, plugin: &PluginId) -> Vec<String> {
    let mut hosts: Vec<String> = host.granted(plugin).iter().filter_map(|capability| host_of(capability)).collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

/// The host a `network:fetch:` capability names, when it names one.
///
/// # Why `*` is not a host
///
/// Because it is how the **gate** spells the family: the generated table answers `network:fetch:*` for the whole group, and a
/// guard that read it as a hostname would be a guard for a machine literally called `*`. A grant that names no host therefore
/// grants no host, which is the fail-closed reading.
#[must_use]
fn host_of(capability: &str) -> Option<String> {
    let rest = capability.strip_prefix("network:fetch:")?;
    let host = rest.split(':').next().unwrap_or_default();
    if host.is_empty() || host == "*" {
        return None;
    }
    Some(host.to_owned())
}

/// The refusal for everything this shell does not implement.
fn not_implemented(group: &str) -> CallError {
    CallError::new(
        CallError::NOT_IMPLEMENTED,
        format!("this shell does not implement {group}"),
    )
}

/// A store failure, as the plugin sees it.
///
/// `hostFailed` rather than something more specific, for the reason the daemon's host gives: the plugin did nothing wrong,
/// and the code tells it not to retry the same call in a loop.
fn failed(what: &str, error: &impl std::fmt::Display) -> CallError {
    CallError::new(CallError::HOST_FAILED, format!("could not {what}: {error}"))
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
        self.platform
            .supported_actions()
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
        //
        // ★ The action's trace is prefixed when the call is part of one, from the thread's scope: `PluginSet::call_traced`
        // makes it current for the call, and the plugin contract has no field to carry it.
        let trace = synra_observe::trace_scope::current()
            .map_or_else(String::new, |trace| format!("trace={trace} "));
        eprintln!(
            "synra: [{level}] {}{trace}{}{message}",
            plugin.as_str(),
            context.map_or(String::new(), |c| format!("({c}) "))
        );
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

    // ★ Storage works, and it is the same table the daemon writes to.
    //
    // These five refused with `notImplemented` until the store grew a `plugin_storage` table. The shell has a database and
    // a plugin set, so there was never a reason for it to be the one host where a plugin could not remember a setting —
    // and a plugin that behaves differently depending on which host runs it is a plugin whose author cannot test it.
    //
    // The quota is checked by `HostCalls` before `storage_set` is reached, against the plugin's own quota; this host
    // reports usage only, because a ceiling is not the host's to decide.
    fn storage_get(&self, plugin: &PluginId, key: &str) -> Result<Option<String>, CallError> {
        self.db
            .lock()
            .storage_get(plugin.as_str(), key)
            .map_err(|error| failed("read the plugin's storage", &error))
    }

    fn storage_set(&self, plugin: &PluginId, key: &str, value: &str) -> Result<(), CallError> {
        self.db
            .lock()
            .storage_set(plugin.as_str(), key, value, synra_store::sqlite::now_ms())
            .map_err(|error| failed("write the plugin's storage", &error))
    }

    fn storage_delete(&self, plugin: &PluginId, key: &str) -> Result<(), CallError> {
        self.db
            .lock()
            .storage_delete(plugin.as_str(), key)
            .map_err(|error| failed("remove the plugin's storage", &error))?;
        Ok(())
    }

    fn storage_keys(&self, plugin: &PluginId, prefix: Option<&str>) -> Result<Vec<String>, CallError> {
        self.db
            .lock()
            .storage_keys(plugin.as_str(), prefix)
            .map_err(|error| failed("list the plugin's storage", &error))
    }

    fn storage_used(&self, plugin: &PluginId) -> Result<u64, CallError> {
        self.db
            .lock()
            .storage_used_bytes(plugin.as_str())
            .map_err(|error| failed("measure the plugin's storage", &error))
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

    fn fetch(&self, plugin: &PluginId, request_json: &str) -> Result<String, CallError> {
        let request: synra_fetch::Request = serde_json::from_str(request_json).map_err(|error| {
            CallError::new(CallError::BAD_ARGUMENT, format!("the request is not readable: {error}"))
        })?;

        let client = fetcher_for(self, plugin)?;
        let response = client.fetch(&request).map_err(|refusal| {
            // The guard's codes are the contract's own and travel as they are: `hostNotAllowed` is what a plugin branches on
            // to try somewhere else, and `responseTooLarge` to ask for less.
            CallError::new(refusal.code.as_str(), refusal.message)
        })?;

        serde_json::to_string(&response).map_err(|error| {
            CallError::new(CallError::HOST_FAILED, format!("the answer could not be written: {error}"))
        })
    }

    fn transfer_send(&self, _p: &PluginId, _k: &str, _t: &str, _m: &str) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn transfer_status(&self, _p: &PluginId, _id: &str) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn action_declare(&self, plugin: &PluginId, name: &str, schema_json: &str) -> Result<(), CallError> {
        // ★ The empty string is "no schema", not "a schema that is empty": `DeclareArguments` defaults the field, so a
        // plugin that declares only a name sends nothing, and storing `""` would make every reader parse a blank.
        let schema = if schema_json.trim().is_empty() {
            None
        }
        else {
            // Validated here so the table holds JSON or nothing. A declaration that is not JSON is the plugin's own
            // error, and the answer names it rather than storing a string every later reader has to guard against.
            serde_json::from_str::<serde_json::Value>(schema_json).map_err(|error| {
                CallError::new(CallError::BAD_ARGUMENT, format!("the schema for `{name}` is not JSON: {error}"))
            })?;
            Some(schema_json)
        };
        if name.trim().is_empty() {
            return Err(CallError::new(CallError::BAD_ARGUMENT, "an action needs a name"));
        }
        self.db
            .lock()
            .record_plugin_action(plugin.as_str(), name, schema, None)
            .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()))
    }

    fn action_list(&self, plugin: &PluginId) -> Result<String, CallError> {
        // The shape `ActionDescriptor` declares: `args` parsed rather than a string, so a page rendering a form does
        // not have to parse a JSON string it was handed inside JSON.
        let rows = self
            .db
            .lock()
            .plugin_actions(plugin.as_str())
            .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()))?;
        let actions: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|row| {
                let mut action = serde_json::Map::new();
                action.insert("name".to_owned(), serde_json::Value::String(row.name));
                if let Some(schema) = row.args_schema
                    && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&schema)
                {
                    action.insert("args".to_owned(), parsed);
                }
                if let Some(description) = row.description {
                    action.insert("description".to_owned(), serde_json::Value::String(description));
                }
                serde_json::Value::Object(action)
            })
            .collect();
        serde_json::to_string(&actions)
            .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()))
    }

    fn action_invoke(&self, _p: &PluginId, _t: &str, _n: &str, _a: &str) -> Result<String, CallError> {
        // ★ Still refused, and now it is the only action call that is: a plugin invoking **another** plugin's action is
        // a cross-plugin permission (`action:invoke:*`), and the path it needs — resolve the target, check the grant,
        // reach a set this host does not own — is not the one `self.invoke` uses. The UI's own call is in
        // `plugin_ui_rpc`, where the set is reachable.
        Err(not_implemented("another plugin's actions"))
    }
}

/// Where a plugin's audit records go, besides the ring.
///
/// # ★ Why the shell needs one, and did not have one
///
/// `PluginSet::with_audit_sink` was written for the daemon, and the daemon was the only host that called it. So in the
/// desktop shell a plugin's audit trail lived in the in-memory ring and **died with the process** — which is the exact gap
/// the sink exists to close, and the reason the `plugin_audit` migration says what the table is for: *"the durable part is
/// what makes the interface's 'recent activity' survive a restart."* A user who quit the app and opened it again saw an
/// empty history, and nothing anywhere said the history had never been written down.
///
/// # Why the write is logged and not refused
///
/// Because the call it describes has already happened. Refusing afterwards would tell a plugin its action did not occur when
/// it did, and an audit trail is a record of what happened rather than a gate on it — the same reasoning the daemon's sink
/// follows.
#[must_use]
pub fn audit_sink(db: Arc<Mutex<SynraDb>>) -> synra_core::dispatch::plugins::AuditSink {
    Arc::new(move |entry: &synra_observe::audit::AuditEntry| {
        let record = synra_store::records::AuditRecord {
            plugin_id: entry.plugin.as_str().to_owned(),
            capability: entry.capability.to_string(),
            outcome: entry.outcome.as_str().to_owned(),
            at_ms: entry.at_ms,
            elapsed_ms: entry.elapsed_ms.map(|elapsed| i64::try_from(elapsed).unwrap_or(i64::MAX)),
            // ★ The action's trace, when the call had one. `can-handle` and lifecycle calls do not, and `None` says that
            // rather than making every row look like part of a user action.
            trace_id: entry.trace.map(|trace| trace.to_string()),
        };
        if let Err(error) = db.lock().record_audit(&record) {
            // The app has no `tracing`, so this is what the shell reports through — the same channel `main.rs` uses for the
            // plugin host's own failures.
            eprintln!(
                "synra: an audit record for {} could not be written down: {error}. The call it describes has already happened.",
                entry.plugin.as_str()
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use synra_plugin_host::host_calls::Host as _;
    use synra_protocol::ids::PluginId;

    use super::{DesktopHost, fetcher_for, host_of};

    /// A plugin identifier, spelled the way the protocol spells it.
    fn id(name: &str) -> PluginId {
        PluginId::parse(name).expect("a plugin id")
    }

    fn host() -> DesktopHost {
        let state = crate::testing::state();
        // ★ The plugin rows have to exist: `plugin_storage` references `plugins`, so a value stored against a plugin this
        // database has never heard of is refused by the schema. That is the cascade doing its job — and it is why the first
        // version of this test failed with `FOREIGN KEY constraint failed`, which is a better answer than silently keeping
        // storage for a plugin nobody installed.
        for plugin in ["@acme/notes", "@acme/chat"] {
            state
                .db
                .lock()
                .insert_plugin(plugin, true, true, "healthy", 1, 1)
                .expect("a plugin row");
        }
        DesktopHost::new(std::sync::Arc::clone(&state.db))
    }

    #[test]
    fn a_capability_names_a_host_or_it_names_nothing() {
        // The parsing rule on its own, because the tests below depend on it being the fail-closed reading.
        assert_eq!(host_of("network:fetch:api.github.com").as_deref(), Some("api.github.com"));
        // A method qualifier is not part of the host: the guard decides *where*, and the method travels as sent.
        assert_eq!(host_of("network:fetch:api.github.com:POST").as_deref(), Some("api.github.com"));

        // ★ `*` is the family spelling the **gate** uses, not a hostname — reading it as one builds a guard for a machine
        // called `*`, and reading it as "anything" is the capability an SSRF guard exists to make deliberate.
        assert_eq!(host_of("network:fetch:*"), None);
        assert_eq!(host_of("network:fetch:"), None);
        assert_eq!(host_of("network:fetch"), None);
        assert_eq!(host_of("fs:read:/data/**"), None);
    }

    #[test]
    fn a_plugin_with_no_host_grant_is_told_which_spelling_it_needs() {
        // Nothing granted: `capabilityDenied`, because the answer is "ask the user" rather than "that host is not one of
        // them" — and a plugin author can act on the first.
        let host = host_with(&[]);
        let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("no grant");

        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED);
        assert!(refused.message.contains("network:fetch:"), "and it says what to ask for: {}", refused.message);
    }

    #[test]
    fn a_family_grant_with_a_star_is_not_a_grant_to_reach_everything() {
        // ★ The whole point of the guard, at the shell: `network:fetch:*` passes the **gate** — it is the family — and it
        // must not become "every host on the internet".
        let host = host_with(&[("network:fetch:*", true)]);
        let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("no host named");

        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED, "{refused}");
    }

    #[test]
    fn a_granted_host_gives_a_client_and_withdrawing_it_takes_the_client_away() {
        // ★ The test the cache needs: a client built for a grant must not outlive the grant. If the cache were keyed on the
        // plugin alone, the call after the withdrawal would hand back the earlier client — and a user who withdrew a host
        // would keep reaching it.
        let host = host_with(&[("network:fetch:api.github.com", true)]);
        assert!(fetcher_for(&host, &id("@acme/notes")).is_ok(), "granted");
        assert!(fetcher_for(&host, &id("@acme/notes")).is_ok(), "and again");

        host.db
            .lock()
            .record_grants("@acme/notes", &[("network:fetch:api.github.com".to_owned(), false)], 2, "d2")
            .expect("the withdrawal is recorded");
        let refused = fetcher_for(&host, &id("@acme/notes")).expect_err("withdrawn");

        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED, "{refused}");
    }

    #[test]
    fn a_plugin_can_store_something_in_the_shell_too() {
        // ★ The same five functions the daemon implements, over the same table.
        //
        // They refused with `notImplemented` here for as long as the store had no table — and a plugin that behaves
        // differently depending on which host runs it is one its author cannot test. The shell has a database, so there
        // was never a reason for it to be the one host where nothing persists.
        //
        // The round trip, because a store that accepts a write and returns nothing is indistinguishable from one that
        // discarded it.
        let host = host();
        let plugin = id("@acme/notes");

        assert_eq!(host.storage_get(&plugin, "k").expect("reads"), None, "an empty store reads as absent");

        host.storage_set(&plugin, "k", "kept").expect("writes");
        assert_eq!(host.storage_get(&plugin, "k").expect("reads").as_deref(), Some("kept"));
        assert_eq!(host.storage_used(&plugin).expect("measures"), 4, "the value's bytes");
        assert_eq!(host.storage_keys(&plugin, None).expect("lists"), vec!["k".to_owned()]);

        // And another plugin's key space is its own, which is what the pair being the primary key buys.
        let other = id("@acme/chat");
        assert_eq!(host.storage_get(&other, "k").expect("reads"), None);

        host.storage_delete(&plugin, "k").expect("removes");
        assert_eq!(host.storage_used(&plugin).expect("measures"), 0);
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
        ] {
            let error = refusal.expect_err("this shell does not implement it");
            // ★ Read from the contract rather than spelled here. This assertion used to compare against the
            // literal `"notImplemented"`, which is a string this test and the implementation could agree on while both were
            // wrong — the shape of assertion that cannot catch the implementation spelling it wrong.
            assert_eq!(error.code, synra_plugin_host::host_calls::CallError::NOT_IMPLEMENTED, "{error:?}");
        }

        // ★ Storage is **not** in that list any more, and its absence is the point.
        //
        // It was the largest group in it and the shell is where a plugin is most likely to run. Asserted positively, so
        // that removing it from the loop cannot be mistaken for removing the feature.
        let plugin = id("@acme/notes");
        host.storage_set(&plugin, "k", "v").expect("writes");
        assert_eq!(host.storage_get(&plugin, "k").expect("reads").as_deref(), Some("v"));
        assert_eq!(host.storage_used(&plugin).expect("measures"), 1);

        // ★ And network is not in that list either. Asserted positively the same way: the refusal below is a **capability**
        // answer rather than a "this shell cannot" one, which is the difference this test exists to protect — and it is
        // reachable without a network, because the guard refuses before anything is sent.
        let refused = host.fetch(&plugin, r#"{"url":"https://api.github.com/"}"#).expect_err("no grant");
        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::CAPABILITY_DENIED, "{refused:?}");

        // ★ Actions are out of the list too, in the half that belongs to this host: a plugin declares its own and lists
        // them. The declaration is validated here — a schema that is not JSON is the plugin's error, not a row every
        // reader has to guard against.
        host.action_declare(&plugin, "send", r#"{"type":"object"}"#).expect("declares");
        let listed = host.action_list(&plugin).expect("lists");
        assert!(listed.contains("\"send\""), "the declaration did not reach the list: {listed}");
        let refused = host.action_declare(&plugin, "broken", "not json").expect_err("not JSON");
        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::BAD_ARGUMENT, "{refused:?}");

        // The one action call that still refuses is **another** plugin's: that is a cross-plugin permission, and the
        // path it needs — resolve the target, check the grant, reach a set this host does not own — is not the one the
        // page's own `self.invoke` uses.
        let refused = host
            .action_invoke(&plugin, "@acme/other", "send", "null")
            .expect_err("this host does not reach another plugin");
        assert_eq!(refused.code, synra_plugin_host::host_calls::CallError::NOT_IMPLEMENTED, "{refused:?}");
    }

    #[test]
    fn an_audited_call_is_written_down_rather_than_only_remembered() {
        // ★ The half the shell was missing. The ring is written by the kernel and read by the running process; this is what
        // makes the trail outlive it. What is asserted is the **table**, read back through the store rather than the entry
        // that was handed in — so a sink that accepted the entry and wrote nothing fails here.
        let db = Arc::new(Mutex::new({
            let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
            // The migrations are not run by `open_in_memory`, and without them the table does not exist — which is what the
            // first version of this test found, as `no such table: plugin_audit` rather than as a passing assertion.
            db.migrate().expect("the schema");
            db
        }));
        let sink = super::audit_sink(Arc::clone(&db));

        let entry = synra_observe::audit::AuditEntry {
            plugin: PluginId::parse("@acme/notes").expect("an id"),
            capability: "device:list".parse().expect("a capability"),
            outcome: synra_observe::audit::AuditOutcome::Allowed,
            at_ms: 1_000,
            elapsed_ms: Some(3),
            trace: None,
        };
        sink(&entry);

        let trail = db.lock().audit_trail().expect("the trail");
        assert_eq!(trail.len(), 1, "the record reached the table");
        assert_eq!(trail[0].plugin_id, "@acme/notes");
        assert_eq!(trail[0].capability, "device:list");
        assert_eq!(trail[0].outcome, "allowed");
        assert_eq!(trail[0].elapsed_ms, Some(3));
    }
}
