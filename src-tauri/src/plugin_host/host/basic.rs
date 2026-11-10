/// Internal host-method expansion.
#[macro_export]
macro_rules! basic_host_methods {
    () => {
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
    };
}
