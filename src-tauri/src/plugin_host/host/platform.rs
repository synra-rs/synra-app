/// Internal host-method expansion.
#[macro_export]
macro_rules! platform_host_methods {
    () => {
        fn platform(
            &self,
            _plugin: &PluginId,
            action: PlatformAction,
            argument: &str,
        ) -> Result<Option<String>, CallError> {
            if action == PlatformAction::OpenPath && argument.starts_with("synra-app://") {
                return open_registered_app(argument.trim_start_matches("synra-app://"))
                    .map(|()| None);
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
            let tracing_keys = action == PlatformAction::SimulateKeys;
            if tracing_keys {
                eprintln!("synra: platform simulate-keys start chord={argument:?}");
            }
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
            if tracing_keys {
                match &outcome {
                    Ok(()) => eprintln!("synra: platform simulate-keys result=ok chord={argument:?}"),
                    Err(error) => eprintln!(
                        "synra: platform simulate-keys result=error chord={argument:?} error={error:?}"
                    ),
                }
            }
            outcome.map_or_else(|error| Err(platform_refusal(&error)), |()| Ok(None))
        }

        // ── Everything below is refused, and says which group it belongs to ────────────────────────────────────────────
        //
        // The rule is the daemon host's rule: a function is answered where this shell genuinely knows, and refused with a
        // code where it does not. Refusing is not a placeholder — a plugin branches on the code, and `notImplemented` is a
        // fact it can act on.
    };
}
