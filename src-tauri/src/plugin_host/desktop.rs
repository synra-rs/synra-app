use super::*;

/// The desktop shell's answer to a plugin's host calls.
pub struct DesktopHost {
    pub(super) input: Option<Arc<synra_core::input::Controller>>,
    pub(super) network: Option<Weak<parking_lot::Mutex<synra_core::Host>>>,
    pub(super) events: Arc<synra_core::plugin_events::EventService>,
    /// The database, for the questions answered from what is recorded: which capabilities the user granted, and the
    /// devices this machine knows.
    pub(super) db: Arc<Mutex<SynraDb>>,
    /// The desktop, over `synra-platform`'s command runner.
    ///
    /// ★ Shared with `AppState` and not owned here. The platform keeps per-instance state — the notification limiter is
    /// the one that matters — and a host with its own copy would let a plugin page's `platform.notify` use an allowance
    /// the headless half had already spent, or the other way round.
    pub(super) platform: Arc<synra_platform::CommandPlatform>,
    /// One fetcher per plugin, and the hosts it was built for.
    ///
    /// ★ Why a cache rather than one client per call: a client holds the plugin's **in-flight count**, so a client built per
    /// call would start from zero every time and `max_concurrent` would never refuse anything. The host list is stored
    /// beside it because the guard is built from the plugin's grants — and a user who withdraws a host must not keep
    /// reaching it through a cached client, so a changed list rebuilds the client.
    pub(super) fetch_clients: Mutex<FetchClients>,
}

/// One fetcher per plugin, and the host list it was built for.
///
/// A `type` rather than the written-out type because clippy was right to complain: this appeared inline in the struct above
/// and again wherever it is read, and a reader has to parse four levels of nesting to learn that it is a map of plugins to
/// *(hosts, client)*.
pub(super) type FetchClients =
    std::collections::HashMap<PluginId, (Vec<String>, Arc<synra_fetch::Client>)>;

impl DesktopHost {
    /// The event service this adapter and its plugin runtime share.
    #[must_use]
    pub fn events(&self) -> Arc<synra_core::plugin_events::EventService> {
        Arc::clone(&self.events)
    }
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
    pub fn with_platform(
        db: Arc<Mutex<SynraDb>>,
        platform: Arc<synra_platform::CommandPlatform>,
    ) -> Self {
        let events = synra_core::plugin_events::EventService::new(Arc::clone(&db));
        Self {
            input: None,
            network: None,
            events,
            db,
            platform,
            fetch_clients: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Attach the shared gated input controller, without creating another host or database.
    pub fn with_input_controller(mut self, controller: Arc<synra_core::input::Controller>) -> Self {
        self.input = Some(controller);
        self
    }

    /// Attach the shell's shared peer host so plugin events can use the same authenticated short connection path as transfers.
    pub fn with_network(mut self, host: &synra_core::SharedHost) -> Self {
        self.network = Some(Arc::downgrade(host));
        self
    }

    /// One setting, as the store has it.
    ///
    /// The same shape as the daemon's host: the value is JSON text in the row, and `locale`/`darkMode` are the two
    /// questions that read it.
    pub(super) fn setting(&self, key: &str) -> Option<String> {
        let rows = self.db.lock().config().ok()?;
        rows.into_iter()
            .find(|row| row.key == key)
            .map(|row| row.value)
    }

    /// The capabilities the user granted this plugin, as the database records them.
    pub(super) fn granted(&self, plugin: &PluginId) -> Vec<String> {
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
pub(super) fn fetcher_for(
    host: &DesktopHost,
    plugin: &PluginId,
) -> Result<Arc<synra_fetch::Client>, CallError> {
    let hosts = granted_hosts(host, plugin);

    // ★ An empty list is refused rather than treated as "any host".
    //
    // `network:fetch:*` is how the **gate** spells the family and a plugin can hold it, but a guard built from it would
    // have to mean "every host on the internet" — exactly the capability an SSRF guard exists to make deliberate. So a grant
    // with no host in it reaches nothing.
    if hosts.is_empty() {
        return Err(CallError::new(
            CallError::CAPABILITY_DENIED,
            format!(
                "`{plugin}` holds no `network:fetch:<host>` grant, so there is nowhere it may fetch from"
            ),
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
    host.fetch_clients
        .lock()
        .insert(plugin.clone(), (hosts, Arc::clone(&client)));
    Ok(client)
}

/// The hosts a plugin was granted for fetching.
///
/// `network:fetch:api.github.com` names a host; `network:fetch:api.github.com:POST` names a host **and a method**, and the
/// method is not this host's to enforce — the client sends whatever the plugin asked for, and the guard decides *where*. A
/// capability that is not `network:fetch:` prefixed, or whose host segment is empty or the literal `*`, contributes nothing.
pub(super) fn granted_hosts(host: &DesktopHost, plugin: &PluginId) -> Vec<String> {
    let mut hosts: Vec<String> = host
        .granted(plugin)
        .iter()
        .filter_map(|capability| host_of(capability))
        .collect();
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
pub(super) fn host_of(capability: &str) -> Option<String> {
    let rest = capability.strip_prefix("network:fetch:")?;
    let host = rest.split(':').next().unwrap_or_default();
    if host.is_empty() || host == "*" {
        return None;
    }
    Some(host.to_owned())
}

/// The refusal for everything this shell does not implement.
pub(super) fn not_implemented(group: &str) -> CallError {
    CallError::new(
        CallError::NOT_IMPLEMENTED,
        format!("this shell does not implement {group}"),
    )
}

/// A store failure, as the plugin sees it.
///
/// `hostFailed` rather than something more specific, for the reason the daemon's host gives: the plugin did nothing wrong,
/// and the code tells it not to retry the same call in a loop.
pub(super) fn failed(what: &str, error: &impl std::fmt::Display) -> CallError {
    CallError::new(CallError::HOST_FAILED, format!("could not {what}: {error}"))
}

/// Turns a platform refusal into a code a plugin can branch on.
///
/// # Why the two are distinguished
///
/// Because they send a plugin author to different places. `badArgument` means the value it supplied is the problem — a
/// `javascript:` URL, a path outside the sandbox, a chord that is not a chord — and the author can fix it. `notImplemented`
/// means the *installation* has no such facility, which no amount of editing will change.
pub(super) fn platform_refusal(error: &synra_platform::PlatformError) -> CallError {
    use synra_platform::PlatformError;
    let code = match error {
        PlatformError::BadUrl { .. }
        | PlatformError::BadPath { .. }
        | PlatformError::TooLong { .. }
        | PlatformError::SimulateKeysNotGranted => CallError::BAD_ARGUMENT,
        PlatformError::Unsupported(_) | PlatformError::Unavailable(_) => CallError::NOT_IMPLEMENTED,
        PlatformError::RunnerBusy { .. } => CallError::RATE_LIMITED,
        PlatformError::NoRecentInteraction { .. }
        | PlatformError::RateLimited { .. }
        | PlatformError::QuotaExceeded { .. }
        | PlatformError::RunnerFailed { .. } => CallError::HOST_FAILED,
        // `PlatformError` is `#[non_exhaustive]`, so a variant added later lands here rather than failing to compile.
        // `hostFailed` is the honest default: telling a plugin its argument was wrong when the fault may be the shell's
        // would be worse than a code it simply retries on.
        _ => CallError::HOST_FAILED,
    };
    let mut refusal = CallError::new(code, error.to_string());
    refusal.details = error.command_details().map(|details| details.to_string());
    refusal
}
