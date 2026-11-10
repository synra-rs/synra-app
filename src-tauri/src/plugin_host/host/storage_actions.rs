/// Internal host-method expansion.
#[macro_export]
macro_rules! storage_actions_host_methods {
    () => {
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
    };
}
