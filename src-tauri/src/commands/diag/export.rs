use super::*;

/// What the loaded plugin set knows about each installed plugin.
///
/// # ★ Why the store cannot answer this
///
/// Because the `plugins` table records `health`, not versions or liveness: it is what the kernel *observed*, written at
/// install time and updated on a health change. "Is it loaded right now" is a fact about this process, and "what version"
/// is a fact about the manifest the loader read — both live in the set, and neither is in the database.
///
/// # Why the lock is taken once
///
/// The set is shared with every plugin call in flight, so a guard taken per row is a guard that blocks a running plugin
/// for as long as this loop takes. One lock, one pass, released before the bundle is assembled.
#[derive(Default)]
struct LoadedPlugin {
    /// The version the loader read, or empty when the set does not have it.
    version: String,
    /// Whether it is loaded right now.
    active: bool,
    /// Whether the ring has refused anything for it.
    had_refusals: bool,
}

/// Reads the set once for every installed plugin.
///
/// A shell with no engine answers "not loaded, no version, nothing refused" for all of them — which is true of a shell
/// that has no engine, and not a fallback.
fn loaded_plugins(
    state: &AppState,
    installed: &[synra_store::records::PluginRow],
) -> Vec<LoadedPlugin> {
    let Ok(set) = state.plugin_set() else {
        return installed.iter().map(|_| LoadedPlugin::default()).collect();
    };
    let set = set.lock();
    installed
        .iter()
        .map(|row| {
            let Ok(id) = synra_protocol::PluginId::parse(&row.plugin_id) else {
                return LoadedPlugin::default();
            };
            LoadedPlugin {
                version: set.version(&id).unwrap_or_default().to_owned(),
                active: set.contains(&id),
                // ★ The one that used to be a hard-coded `false`.
                had_refusals: !set.refusals_for(&id).is_empty(),
            }
        })
        .collect()
}

/// Assembles a diagnostic bundle and writes it.
///
/// # Errors
///
/// * [`IpcError::Internal`] when the configuration or the devices cannot be read, or the file cannot be written.
///
/// # Why this reads the store directly rather than going through [`super::config_get`]
///
/// Because a bundle is exactly the moment when the *whole* picture is wanted, including the parts that are inconvenient
/// — so it is the one command that should not stop at the first bad row. A corrupt setting must not be the reason a
/// user cannot get help; the value goes in as the text it is, and the omission list says it was unreadable.
pub fn diag_export(
    state: &AppState,
    options: &DiagExportOptions,
) -> Result<DiagExportResult, IpcError> {
    let (devices, plugins) = {
        let db = state.db();
        (
            db.paired_devices().map_err(internal)?,
            db.plugins().map_err(internal)?,
        )
    };

    // ★ The plan is built with the sections this shell can fill, and the ones it cannot are stated as omissions.
    //
    // ★ Three of the four plugin fields used to be constants — `version: String::new()`, `active: false`,
    // `had_refusals: false` — and that was not an omission, it was **four claims**. `had_refusals: false` is the one
    // that matters: it tells a maintainer that nothing this plugin tried was ever refused, in the document a user sends
    // when they are asking why something did not work. A refusal is usually the answer.
    //
    // All three are readable now. The set knows what this process managed to load and which version; its ring knows
    // whether anything was refused.
    let loaded = loaded_plugins(state, &plugins);
    let mut sources = BundleSources {
        environment: Some(environment()),
        config: Some(configuration(state)),
        devices: Some(
            devices
                .iter()
                .map(|row| DeviceRecord {
                    device_id: row.device_id.clone(),
                    paired: true,
                    // ★ Asked rather than assumed. Every device here is paired by construction — this is the paired table —
                    // but "paired" and "reachable" are different, and a bundle that reported every peer online would send
                    // a maintainer looking for a network fault that is not there.
                    online: row.is_usable(),
                })
                .collect(),
        ),
        plugins: Some(
            plugins
                .iter()
                .zip(loaded)
                .map(|(row, state)| PluginRecord {
                    plugin_id: row.plugin_id.clone(),
                    version: state.version,
                    active: state.active,
                    had_refusals: state.had_refusals,
                })
                .collect(),
        ),
        ..BundleSources::default()
    };

    // ★ The optional sections are given a source only when the user asked for them.
    //
    // ★ The log is real, and so is the audit trail now — but the two are read from different places, because they are
    // written by different things. The shell starts its own log (`AppState::start_logging`), so `log_lines` comes from the
    // file the shell is writing. The audit trail comes from the plugin set's ring, which every plugin instance drains into
    // after each authorised call.
    //
    // ★ This said "there is still no audit sink" for several rounds after that stopped being true. An empty list was
    // honest when nothing wrote one; it is a false answer now, because a bundle that says it carries the audit trail and
    // carries none tells a user their plugins did nothing.
    if options.include_audit {
        // No engine means no instances, so no calls were authorised and an empty trail is the truth rather than a
        // fallback. A refusal here would be worse: the section is still legitimately empty.
        let recorded = state
            .plugin_set()
            .map(|set| set.lock().audit())
            .unwrap_or_default();
        sources.audit = Some(recorded);
    }
    if options.include_debug_logs || options.log_lines.is_some() {
        // `verbose` is true because the user asking for debug logs has asked for the debug lines; the quiet filter would
        // hide exactly what they requested, and the bundle would carry an empty section that looked deliberate.
        //
        // ★ The shell's writer is asynchronous, so the tail is asked for **after** a flush: a bundle assembled seconds
        // after the failure is exactly the case where the line explaining it is still in the queue, and a bundle missing
        // that line sends the reader looking for a different explanation.
        if let Some(guard) = state.log.as_ref() {
            guard.flush();
        }
        let wanted = options
            .log_lines
            .map_or(synra_observe::logging::DEFAULT_TAIL_LINES, |n| n as usize);
        sources.log_lines =
            Some(synra_observe::logging::tail(state.paths.log_dir(), wanted, true, None).lines);
    }

    let mut plan = BundlePlan::plan(
        sources,
        state.started_at_ms,
        destination(state, state.next_bundle_id()),
    );
    if let Some(lines) = options.log_lines {
        plan.set_log_tail_lines(lines as usize);
    }
    // ★ Consent switches a section on, not merely makes one available.
    //
    // The plan's defaults are the sections a bundle always carries. Providing a source for an optional section is not
    // enough — without this the option would be accepted, the source would be built, and the section would still be
    // dropped, which is a consent screen that agrees with the file about nothing.
    if options.include_audit {
        plan.toggle_optional(BundleSection::AuditTrail, true);
    }
    if options.include_debug_logs {
        plan.toggle_optional(BundleSection::DebugLogs, true);
    }

    let rendered = plan.render().map_err(internal)?;
    let bundle = plan.build();

    // ★ The directory is made here, not at startup.
    //
    // A shell that creates it once on launch depends on that having happened: the user clears the directory, moves the
    // data folder, or installs onto a machine where the launch path never ran — and "export a diagnostic bundle" then
    // fails at exactly the moment a user most needs it to work. Making it at the point of use costs one call and cannot
    // be broken by anything that happened before.
    let path = plan.destination().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(internal)?;
    }
    std::fs::write(&path, rendered).map_err(internal)?;

    Ok(DiagExportResult {
        path: path.display().to_string(),
        included: bundle.sections.clone(),
        excluded: bundle.excluded.clone(),
    })
}
