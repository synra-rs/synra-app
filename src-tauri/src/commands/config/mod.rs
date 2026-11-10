//! The configuration, as the settings screens read and write it.
//!
//! Two commands, and the pairing of them is the point: a screen that can read settings it cannot write is a screen that
//! looks broken, and one that can write without reading back is a screen that silently disagrees with the next start.

use synra_ipc::{ConfigPatch, ConfigSnapshot, IpcError};
use synra_protocol::ErrorCode;
use synra_store::ConfigChange;

use crate::state::AppState;

/// Reads the configuration.
///
/// # Errors
///
/// * [`IpcError::Internal`] when the rows cannot be read.
/// * [`IpcError::InvalidArgument`] when a stored value is not JSON text.
///
/// # Why a malformed value is a refusal rather than being skipped
///
/// Because a settings screen that shows eight of nine settings, with no indication that the ninth is broken, is a screen
/// whose user will turn the ninth one on and find it does not take. Naming the key is the only thing that makes it
/// diagnosable, and the key is the one thing that survives the value being unreadable.
pub fn config_get(state: &AppState) -> Result<ConfigSnapshot, IpcError> {
    let rows = {
        let db = state.db();
        db.config().map_err(|error| internal(&error))?
    };

    let mut values = serde_json::Map::new();
    let mut overridden = Vec::new();
    for row in &rows {
        values.insert(row.key.clone(), row_value(row)?);
        // ★ Every stored key is an override.
        //
        // A row exists because a file or a screen put one there, so the list is the row keys rather than a separate
        // record of provenance that could disagree with them.
        overridden.push(row.key.clone());
    }

    Ok(ConfigSnapshot {
        values: serde_json::Value::Object(values),
        overridden,
    })
}

/// Turns one row's stored text into a value, naming the key if it cannot.
///
/// # Why this is a function and not inline
///
/// Because the row it has to handle — text in the column that does not parse — cannot be produced through any API this
/// crate has. The writer always serialises. So a test can only reach this by calling it with a row it built by hand, and
/// a rule that cannot be reached is a rule that was never checked.
fn row_value(row: &synra_store::ConfigRow) -> Result<serde_json::Value, IpcError> {
    serde_json::from_str(&row.value).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "key": row.key,
            "reason": format!("the stored value is not readable: {error}"),
        }))
    })
}

/// Writes a patch.
///
/// # Errors
///
/// * [`IpcError::InvalidArgument`] when the patch sets and unsets the same key, or is empty of keys to write.
///
/// The conflict is refused rather than resolved by precedence: a caller that asked for both meant one of them, and
/// picking for it produces a setting that is the opposite of what it wanted half the time. The kernel's own
/// [`ConfigPatch::has_conflict`] is what decides, so there is one rule rather than one per host.
///
/// An empty patch is *accepted* rather than refused — a settings screen sends one when it opens, before the user has
/// touched anything, and answering that with an error makes the screen look broken on load.
pub fn config_set(state: &AppState, patch: &ConfigPatch) -> Result<(), IpcError> {
    if let Some(key) = patch.has_conflict() {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"key": key, "reason": "a key cannot be both set and unset"}),
        ));
    }

    if let Some(value) = patch.set.get(super::plugin_updates::AUTO_UPDATE_KEY)
        && !matches!(
            value.as_str(),
            Some("never" | "notify-only" | "auto-minor" | "auto-all")
        )
    {
        return Err(
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "key": super::plugin_updates::AUTO_UPDATE_KEY,
                "reason": "select never, notify-only, auto-minor or auto-all",
            })),
        );
    }

    if let Some(value) = patch.set.get(super::transfers::ACCEPT_FROM_KEY) {
        super::transfers::validate_accepted_senders(value)?;
    }

    let changes: Vec<ConfigChange> = patch
        .set
        .iter()
        .map(|(key, value)| ConfigChange {
            key: key.clone(),
            value: Some(value.clone()),
        })
        .chain(patch.unset.iter().map(|key| ConfigChange {
            key: key.clone(),
            value: None,
        }))
        .collect();

    {
        let db = state.db();
        db.apply_config(&changes, state.started_at_ms)
            .map_err(|error| internal(&error))?;
    }

    // Cached updates belong to the registry that answered them. Re-publish the cards after changing sources so an
    // already open plugins page drops the old answers as soon as the configuration write succeeds.
    if changes
        .iter()
        .any(|change| change.key == super::registry::REGISTRY_KEY)
    {
        let rows = state.db().plugins().map_err(|error| internal(&error))?;
        for row in rows {
            let id = synra_protocol::PluginId::parse(&row.plugin_id).map_err(|error| {
                IpcError::unknown(ErrorCode::Internal)
                    .with_params(serde_json::json!({"detail": error.to_string()}))
            })?;
            super::plugins::announce(state, &id);
        }
    }

    // ★ Read back, and compared.
    //
    // A write that matched no row reports success — SQLite does not tell a `DELETE` that found nothing from one that
    // found a row. Re-reading is what turns "the statement ran" into "the setting is there", and it is cheap enough here
    // to be worth the certainty. Only removals are checked, because a value the store refused to serialise is already an
    // error rather than a silent success.
    if patch.unset.is_empty() {
        // ★ Announced on **both** paths, and this is the common one: a patch that only sets values returns here, so a
        // publish at the end of the function would never run for the case the interface uses most.
        announce_config(state);
        return Ok(());
    }
    let remaining = {
        let db = state.db();
        db.config().map_err(|error| internal(&error))?
    };
    let still_there: Vec<&String> = remaining
        .iter()
        .filter(|row| patch.unset.iter().any(|key| key == &row.key))
        .map(|row| &row.key)
        .collect();
    if !still_there.is_empty() {
        return Err(IpcError::unknown(ErrorCode::Internal).with_params(
            serde_json::json!({"keys": still_there, "reason": "the keys were not removed"}),
        ));
    }
    // ★ Announced here as well as in the early-return above: this path is a patch that also removed something, and the
    // order of the two calls is what keeps "refused" from being announced — every refusal above returned before either.
    announce_config(state);
    Ok(())
}

/// Announces the configuration as it now stands.
///
/// Silent when the read-back fails: an event is not where an error goes, and the next `config_get` is the truth either way.
pub(crate) fn announce_config(state: &AppState) {
    if let Ok(snapshot) = config_get(state) {
        state.publish(synra_ipc::event::KernelEvent::ConfigUpdated(Box::new(
            snapshot,
        )));
    }
}

/// The one place a database failure becomes an `IpcError`, so every command here reports it identically.
///
/// By reference because the message is taken and the error is then dropped: taking it by value would move a
/// [`DbError`] nobody can use again, for a message that is built and immediately consumed.
fn internal(error: &synra_store::DbError) -> IpcError {
    IpcError::unknown(ErrorCode::Internal)
        .with_params(serde_json::json!({ "reason": error.to_string() }))
}

#[cfg(test)]
mod tests;
