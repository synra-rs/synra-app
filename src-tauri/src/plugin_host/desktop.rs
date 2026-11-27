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
    /// Shared network budgets for page and Wasm calls.
    pub(super) fetch_clients: synra_fetch::PluginClients,
}

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
            fetch_clients: synra_fetch::PluginClients::default(),
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
