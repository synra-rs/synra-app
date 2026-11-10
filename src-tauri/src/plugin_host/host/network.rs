/// Internal host-method expansion.
#[macro_export]
macro_rules! network_host_methods {
    () => {
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
    };
}
