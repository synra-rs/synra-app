use super::*;

impl Host for DesktopHost {
    fn input_session(&self, plugin: &PluginId, request: &str) -> Result<String, CallError> {
        self.input
            .as_ref()
            .ok_or_else(|| CallError::unknown("input-session"))?
            .request(plugin, request)
            .map_err(|reason| CallError::new(CallError::BAD_ARGUMENT, reason))
    }

    fn sdk_version(&self) -> String {
        synra_plugin_host::SDK_VERSION.to_owned()
    }

    fn host_kind(&self) -> String {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            "mobile"
        } else {
            "desktop"
        }
        .to_owned()
    }

    fn locale(&self) -> String {
        // ★ The interface's own setting, the same key `config_set` writes and `plugin_ui`'s injection reads. Returning
        // an empty string made `activate(config.locale)` and `env.locale()` disagree with the language the window is
        // actually drawn in.
        let preference = self
            .setting("ui.locale")
            .and_then(|value| serde_json::from_str::<String>(&value).ok());
        crate::locale::from_preference(preference.as_deref())
            .code()
            .to_owned()
    }

    fn dark_mode(&self) -> bool {
        self.setting("ui.theme")
            .and_then(|value| serde_json::from_str::<String>(&value).ok())
            .is_some_and(|theme| theme == "dark")
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
        let mut capabilities = self
            .platform
            .supported_actions()
            .iter()
            .map(|action| action.capability().to_owned())
            .chain(
                synra_platform::pointer::supported()
                    .then(|| synra_platform::pointer::CAPABILITY.to_owned()),
            )
            .collect::<Vec<_>>();
        capabilities.extend(registered_apps().iter().map(app_capability));
        capabilities
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
                (
                    device.device_id.clone(),
                    device.display_name.clone(),
                    device.platform.clone(),
                )
            })
            .collect()
    }

    fn platform(
        &self,
        _plugin: &PluginId,
        action: PlatformAction,
        argument: &str,
    ) -> Result<Option<String>, CallError> {
        if action == PlatformAction::OpenPath && argument.starts_with("synra-app://") {
            return open_registered_app(argument.trim_start_matches("synra-app://")).map(|()| None);
        }
        if action == PlatformAction::OpenPath && argument == "synra-selection://copy" {
            let selected = self
                .platform
                .read_selected_clipboard()
                .map_err(|error| platform_refusal(&error))?;
            return serde_json::to_string(&selected)
                .map(Some)
                .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()));
        }
        // A notification carries a title and a body separated by the contract's own delimiter, which is the shape
        // `host_calls` builds when it sends one.
        let outcome = match action {
            PlatformAction::OpenUrl => self.platform.open_url(argument),
            PlatformAction::OpenPath => self.platform.open_path(std::path::Path::new(argument)),
            PlatformAction::RevealPath => self
                .platform
                .reveal_path(std::path::Path::new(argument), None),
            PlatformAction::SimulateKeys => self.platform.simulate_keys(argument),
            PlatformAction::ReadClipboard => {
                return self
                    .platform
                    .read_clipboard()
                    .map_err(|error| platform_refusal(&error));
            }
            PlatformAction::WriteClipboard => self.platform.write_clipboard(argument),
            PlatformAction::Notify => {
                let (title, body) = argument.split_once('\u{1}').unwrap_or((argument, ""));
                self.platform.notify(title, body)
            }
            _ => {
                return Err(CallError::new(
                    CallError::NOT_IMPLEMENTED,
                    "this shell does not implement this platform action",
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

    fn event_emit(&self, plugin: &PluginId, event: &str, payload: &str) -> Result<(), CallError> {
        self.events.emit_call(plugin, event, payload)
    }

    fn device_send(
        &self,
        _p: &PluginId,
        target: &str,
        event: &str,
        payload: &str,
    ) -> Result<(), CallError> {
        let Some(host) = self.network.as_ref().and_then(Weak::upgrade) else {
            return Err(not_implemented("device sends"));
        };
        let target = target
            .parse::<DeviceId>()
            .map_err(|error| CallError::new(CallError::BAD_ARGUMENT, error.to_string()))?;
        let event = EventName::parse(event)
            .map_err(|error| CallError::new(CallError::BAD_ARGUMENT, error.to_string()))?;
        let payload = serde_json::value::RawValue::from_string(payload.to_owned())
            .map_err(|error| CallError::new(CallError::BAD_ARGUMENT, error.to_string()))?;
        let trace_id = synra_observe::trace_scope::current().unwrap_or_default();
        let event_label = event.as_str().to_owned();
        let address = crate::commands::peers::resolve(&self.db, &host, target)
            .map_err(|error| CallError::new(CallError::HOST_FAILED, format!("{error:?}")))?;
        let (dial, from) = {
            let guard = host.lock();
            (guard.dialling_parts(), guard.device_id())
        };
        eprintln!("synra: plugin device send queued target={target} event={event}");
        let spawned = std::thread::Builder::new()
            .name("synra-plugin-device-send".to_owned())
            .spawn(move || {
                let now = synra_store::sqlite::now_ms();
                let result = (|| -> Result<(), String> {
                    eprintln!(
                        "synra: plugin device send dialing target={target} event={event_label}"
                    );
                    let dialled = synra_core::dial_peer(
                        &dial.0,
                        dial.1,
                        address,
                        target,
                        synra_core::reading_millis(now),
                    )
                    .map_err(|error| error.to_string())?;
                    eprintln!(
                        "synra: plugin device send connected target={target} event={event_label}"
                    );
                    let mut guard = host.lock();
                    let mut link = guard
                        .adopt(dialled, synra_core::reading_millis(now))
                        .map_err(|error| error.to_string())?;
                    guard.may_send(&link).map_err(|error| error.to_string())?;
                    drop(guard);
                    let envelope = Envelope::outbound(
                        OutboundSpec {
                            event,
                            target,
                            from,
                            trace_id,
                            sent_at_ms: now,
                            ttl_ms: None,
                        },
                        Some(&payload),
                    )
                    .map_err(|error| error.to_string())?;
                    link.send(&envelope).map_err(|error| error.to_string())?;
                    host.lock().count_sent();
                    eprintln!("synra: plugin device send sent target={target} event={event_label}");
                    let _ = link.bye("plugin event delivered");
                    Ok(())
                })();
                if let Err(error) = result {
                    eprintln!("synra: plugin device send failed: {error}");
                }
            })
            .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()))?;
        drop(spawned);
        Ok(())
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

    fn storage_keys(
        &self,
        plugin: &PluginId,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, CallError> {
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
        let client = fetcher_for(self, plugin)?;
        client.fetch_plugin(request_json, &self.granted(plugin))
    }

    fn transfer_send(
        &self,
        _p: &PluginId,
        _k: &str,
        _t: &str,
        _m: &str,
    ) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn transfer_status(&self, _p: &PluginId, _id: &str) -> Result<String, CallError> {
        Err(not_implemented("transfers"))
    }

    fn action_declare(
        &self,
        plugin: &PluginId,
        name: &str,
        schema_json: &str,
    ) -> Result<(), CallError> {
        // ★ The empty string is "no schema", not "a schema that is empty": `DeclareArguments` defaults the field, so a
        // plugin that declares only a name sends nothing, and storing `""` would make every reader parse a blank.
        let schema = if schema_json.trim().is_empty() {
            None
        } else {
            // Validated here so the table holds JSON or nothing. A declaration that is not JSON is the plugin's own
            // error, and the answer names it rather than storing a string every later reader has to guard against.
            serde_json::from_str::<serde_json::Value>(schema_json).map_err(|error| {
                CallError::new(
                    CallError::BAD_ARGUMENT,
                    format!("the schema for `{name}` is not JSON: {error}"),
                )
            })?;
            Some(schema_json)
        };
        if name.trim().is_empty() {
            return Err(CallError::new(
                CallError::BAD_ARGUMENT,
                "an action needs a name",
            ));
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
                    action.insert(
                        "description".to_owned(),
                        serde_json::Value::String(description),
                    );
                }
                serde_json::Value::Object(action)
            })
            .collect();
        serde_json::to_string(&actions)
            .map_err(|error| CallError::new(CallError::HOST_FAILED, error.to_string()))
    }

    fn action_invoke(
        &self,
        _p: &PluginId,
        _t: &str,
        _n: &str,
        _a: &str,
    ) -> Result<String, CallError> {
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
            elapsed_ms: entry
                .elapsed_ms
                .map(|elapsed| i64::try_from(elapsed).unwrap_or(i64::MAX)),
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
