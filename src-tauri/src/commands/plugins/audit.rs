use super::*;

/// What a plugin tried to do, most recent last.
///
/// # ★ Why this command was waiting, and why it no longer is
///
/// The list carried it with the reason *"the audit ring has no production writer yet"*. That was true when it was
/// written: `AuditRing` was bounded, tested and queryable, and **nothing wrote to it** — the kernel's own
/// `record_action` had no host that constructs a kernel, and every plugin instance ran with no session, so no call was
/// ever authorised and none was ever recorded.
///
/// The writer exists now: a `PluginSet` installs a session on every instance it creates, drains what the session
/// recorded into its ring, and `audit_for` reads it back. So this is a read of a real trail rather than of an empty one.
///
/// # Why the last `limit` rather than the first
///
/// Because a settings screen shows what a plugin has been doing **lately**, and the interesting end of a trail is the
/// recent one. `limit` of zero returns nothing, which is what a caller asking for nothing gets.
///
/// # ★ Why there is an offset as well
///
/// Because the table holds the whole retention window, and a card that could only ever show the newest twenty rows made
/// "view the full history" a button the design asked for and this build did not have. The offset is applied in SQL, so
/// "show more" walks the index rather than re-reading the rows the user has already seen.
///
/// # ★ Why this reads the table rather than the ring
///
/// The ring is this process's last ten thousand calls; the table is the history. Both hosts install an audit sink
/// ([`crate::plugin_host::audit_sink`]) that writes every drained record, so the table is a **superset** of the ring —
/// it carries the calls from before the last restart. Reading the ring would answer "nothing" on a fresh start even though
/// the user can still see yesterday's refusals in a diagnostic bundle, and the design's whole reason for the durable half
/// is that "recent activity" survives a restart.
///
/// It also stops needing a plugin engine: the trail outlives the engine, so a build without one can still answer what
/// this database recorded.
///
/// # Errors
///
/// [`IpcError`] when the table cannot be read.
#[allow(clippy::result_large_err)]
pub fn plugins_audit(
    state: &AppState,
    plugin_id: &PluginId,
    limit: u32,
    offset: u32,
) -> Result<Vec<AuditEntryDto>, IpcError> {
    let records = state
        .db()
        .audit_trail_for(plugin_id.as_str(), offset, limit)
        .map_err(internal)?;
    Ok(records.iter().map(record_card).collect())
}

/// Forgets one plugin's audit history.
///
/// # Why the durable rows and the ring
///
/// They are two copies of one trail. Clearing only the table would let the diagnostics bundle — which reads the ring —
/// resurrect what the user just deleted; clearing only the ring would bring the rows back on the next read. A build
/// without an engine has no ring, so that half is skipped rather than refused.
///
/// # Errors
///
/// [`IpcError`] when the table refuses the delete. The ring cannot fail.
#[allow(clippy::result_large_err)]
pub fn plugins_audit_clear(state: &AppState, plugin_id: &PluginId) -> Result<(), IpcError> {
    if let Ok(set) = state.plugin_set() {
        set.lock().clear_audit(plugin_id);
    }
    state
        .db()
        .forget_audit_for(plugin_id.as_str())
        .map_err(internal)?;
    Ok(())
}

/// The configuration key that sets how long recorded calls are kept, in days.
///
/// The design names the setting `audit_retention_days`; the configuration addresses keys by dotted path, and every other
/// key this shell reads is camelCase after the first segment (`transfers.confirmCancel`), so this one is spelled the same
/// way.
pub(crate) const AUDIT_RETENTION_KEY: &str = "audit.retentionDays";

/// How long a recorded call is kept when nothing says otherwise.
const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 7;

/// The shortest and longest retention a caller can ask for.
///
/// A day is the smallest unit the setting has; ten years is past the point where the table is a diagnostic aid and into
/// "why is this database a gigabyte". The clamp is here rather than in the screen because a configuration file can be
/// edited by hand, and a negative or absurd number must not become an arithmetic overflow in the prune.
const MIN_AUDIT_RETENTION_DAYS: u32 = 1;
const MAX_AUDIT_RETENTION_DAYS: u32 = 3_650;

/// The configured retention, clamped, or the design's default when the key is absent or unreadable.
///
/// # Why an unreadable value is the default rather than a refusal
///
/// Because this is read on the start-up path, and a bad setting in a file must not stop the window from opening. The
/// screen that writes it round-trips `config_get`, so the user sees the stored value; a value this function cannot read is
/// one the screen cannot have produced.
fn audit_retention_days(state: &AppState) -> u32 {
    let stored = {
        let db = state.db();
        db.config().ok().and_then(|rows| {
            rows.into_iter()
                .find(|row| row.key == AUDIT_RETENTION_KEY)
                .and_then(|row| serde_json::from_str::<u64>(&row.value).ok())
        })
    };
    match stored {
        Some(days) => u32::try_from(days)
            .unwrap_or(u32::MAX)
            .clamp(MIN_AUDIT_RETENTION_DAYS, MAX_AUDIT_RETENTION_DAYS),
        None => DEFAULT_AUDIT_RETENTION_DAYS,
    }
}

/// Forgets recorded calls older than the retention window, and answers how many went.
///
/// Called once per [`AppState`](crate::state::AppState), like the registry document prune: a table that only grows costs
/// more than the history is worth, and no request should have to remember to sweep it.
pub(crate) fn prune_audit(state: &AppState, now_ms: i64) -> usize {
    let days = audit_retention_days(state);
    let window_ms = i64::from(days) * 24 * 60 * 60 * 1_000;
    match state
        .db()
        .forget_audit_before(now_ms.saturating_sub(window_ms))
    {
        Ok(removed) => removed,
        Err(error) => {
            // A failure to prune is a diagnostic, not a reason to refuse to start.
            eprintln!("synra: recorded calls could not be pruned: {error}");
            0
        }
    }
}

/// One recorded call, as the interface shows it.
///
/// ★ The row stores the outcome's **name**, so this has to read it back. [`AuditOutcome::from_stable_name`] is that
/// parser, and it lives beside `as_str` so the table's spelling has one definition rather than two.
///
/// A name this build does not know is reported rather than guessed at: `allowed: false` with the name as the reason.
/// Presenting an outcome from a newer build as "allowed" is the one answer that could hide a refusal.
///
/// ★ The DTO's `trace_id` now comes from the row. It used to be `None` because the row had no such column and the session
/// that made the call was never given the action's trace — two halves of one gap, closed together: `call_traced` stamps the
/// drained entries, the migration adds the column, and this is the mapping that finally has something to read.
pub(crate) fn record_card(record: &synra_store::records::AuditRecord) -> AuditEntryDto {
    let outcome = synra_observe::audit::AuditOutcome::from_stable_name(&record.outcome);
    AuditEntryDto {
        at_ms: record.at_ms,
        capability: record.capability.clone(),
        allowed: outcome.is_some_and(synra_observe::audit::AuditOutcome::was_allowed),
        // The outcome's stable name, for a refusal only. An allowed call has no reason to give.
        reason: match outcome {
            Some(outcome) if outcome.is_refusal() => Some(outcome.as_str().to_owned()),
            Some(_) => None,
            None => Some(record.outcome.clone()),
        },
        elapsed_ms: record
            .elapsed_ms
            .and_then(|elapsed| u64::try_from(elapsed).ok()),
        trace_id: record.trace_id.clone(),
    }
}

/// Records one finished install pipeline, labelled with where it ended.
///
/// `stage` is the refusal's own stable name (`source`, `manifest`, `verify`, …) or `complete`; `result` is `ok` or
/// `refused`. One sample per attempt, not per step: what the counter answers is "where do installs stop", and a step
/// that succeeded before a later one refused would otherwise make every attempt look like a pass.
pub(crate) fn record_install(state: &AppState, stage: &str, result: &str) {
    state
        .metrics
        .counter_inc(&synra_observe::metrics::SeriesKey::new(
            "plugin_install_total",
            [
                ("result".to_owned(), result.to_owned()),
                ("stage".to_owned(), stage.to_owned()),
            ],
        ));
}

/// The bytes under a directory, or zero when it cannot be read.
///
/// ★ Errors become zero rather than failing the list: a size is a diagnostic, and a plugin whose sandbox is unreadable is
/// still a plugin the list can describe. A directory that does not exist is zero too — "no data yet" and "no permission"
/// are the same number here, and the gauge is not the place to tell them apart.
pub(crate) fn directory_size(root: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_size(&entry.path()),
            Ok(_) => entry.metadata().map_or(0, |metadata| metadata.len()),
            Err(_) => 0,
        })
        .fold(0u64, u64::saturating_add)
}
