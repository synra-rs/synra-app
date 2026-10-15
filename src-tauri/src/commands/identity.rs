//! Who this device is.
//!
//! Two commands, and they exist together because a device that cannot be renamed is one whose identity users report as
//! broken on the other side: "the wrong name" is only diagnosable if this side can show and change its own.

use synra_ipc::{IpcError, IdentitySnapshot};

use crate::state::AppState;

/// What this device is called.
///
/// # Errors
///
/// Never in practice. A snapshot is read from memory and copied, so there is no storage and no lock that can fail; the
/// signature stays `Result` because a command that cannot fail today and starts failing after a refactor would be a
/// change to every caller, and a `Result` that later gets used is cheaper than a `Result` that later has to be added.
pub fn identity_get(state: &AppState) -> Result<IdentitySnapshot, IpcError> {
    Ok(state.identity_snapshot(KERNEL_VERSION))
}

/// Renames this device.
///
/// # Errors
///
/// * [`IpcError::InvalidArgument`] when the name is empty, longer than [`synra_core::MAX_DISPLAY_NAME_LEN`], or has no printable
///   character in it. The kernel's own [`RenameError`] carries the reason; it is mapped rather than re-typed so the
///   kernel stays the single place that decides what a valid name is.
/// * [`IpcError::Internal`] when the rename cannot be written.
pub fn identity_set_display_name(state: &AppState, name: &str) -> Result<(), IpcError> {
    // ★ Checked before the lock, and against the kernel's own rule rather than a copy of it.
    //
    // `check_display_name` is what the kernel enforces for a CLI writing the same row, so re-typing the rule here is a
    // second implementation that can disagree. The cheap `is_empty` before it is not a second rule — it turns the common
    // case into a refusal without building an error the kernel would also produce, and it is a subset of what the kernel
    // refuses.
    if name.is_empty() {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": "name", "reason": "a name cannot be empty"})),
        );
    }
    synra_core::check_display_name(name).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({
                "field": "name",
                "reason": error.to_string(),
                // ★ Carried so the interface can set `maxlength` rather than discovering the limit by being refused.
                "maxChars": synra_core::MAX_DISPLAY_NAME_LEN,
            }))
    })?;

    // ★ The identity lock does not span the registry update.
    //
    // `identity_snapshot` takes this same lock on every command that reads the identity, and holding it across the host
    // lock would nest two locks whose order nothing else enforces — which is the shape that deadlocks the first time
    // some other path takes them the other way round.
    {
        let mut identity = state.identity.lock();
        // The write goes through the kernel rather than through the field, because the kernel is what stores it.
        identity
            .rename(&state.db(), name, state.started_at_ms)
            .map_err(|error| {
                IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
                    .with_params(serde_json::json!({
                    "field": "name",
                    "reason": error.to_string(),
                    // ★ Carried so the interface can set `maxlength` rather than discovering the limit by being refused.
                    "maxChars": synra_core::MAX_DISPLAY_NAME_LEN,
                }))
            })?;
    }

    // ★ The registry is told, and it is told on every path that succeeded.
    //
    // The host holds this device's name in two places and only one of them is in `identity`. A rename that reaches one
    // leaves two devices that disagree about each other's name until something restarts — the kind of fault that gets
    // reported as "it only fixes itself when I reopen the app".
    if let Err(error) = state.host.lock().rename_self(name) {
        return Err(crate::state::internal(error));
    }
    Ok(())
}

/// The version this shell reports as the kernel's.
///
/// Reported from the shell's own crate rather than from a kernel crate, because the shell is what the user installed
/// and a version that came from a transitive path dependency is a version they cannot match against a bug report.
const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether a name is the one the kernel chose.
///
/// Exposed because the identity snapshot needs it and because a test needs to be able to make one without knowing the
/// default's text.
#[must_use]
pub fn is_generated(name: &str) -> bool {
    name == synra_core::DEFAULT_DISPLAY_NAME
}

#[cfg(test)]
mod tests {
    use super::{identity_get, identity_set_display_name, is_generated};
    use crate::testing;
    use synra_protocol::ErrorCode;

    #[test]
    fn a_fresh_device_has_a_name_and_is_marked_as_generated() {
        let state = testing::state();
        let snapshot = identity_get(&state).expect("an identity");
        assert!(snapshot.name_is_default, "a fresh device's name was generated");
        assert!(is_generated(&snapshot.display_name), "the snapshot agrees with the rule");
    }

    #[test]
    fn a_rename_is_visible_through_the_same_command_that_read_it() {
        let state = testing::state();
        identity_set_display_name(&state, "Kitchen tablet").expect("a rename");
        let snapshot = identity_get(&state).expect("an identity");
        assert_eq!(snapshot.display_name, "Kitchen tablet");
        assert!(!snapshot.name_is_default, "a chosen name is not a generated one");
    }

    #[test]
    fn a_rename_survives_a_restart() {
        // ★ Over a database on disk. An in-memory one has nothing on the other side to reopen, so a restart test written
        // against it reads a fresh identity and fails for a reason that has nothing to do with the rename.
        let (root, state) = testing::file_state();
        identity_set_display_name(&state, "Kitchen tablet").expect("a rename");
        let reopened = testing::reopen(&state);
        assert_eq!(identity_get(&reopened).expect("an identity").display_name, "Kitchen tablet");
        drop(root);
    }

    #[test]
    fn a_rename_reaches_the_registry_so_peers_stop_seeing_the_old_name() {
        let state = testing::state();
        identity_set_display_name(&state, "Kitchen tablet").expect("a rename");
        assert_eq!(
            state.host.lock().announced_name(),
            "Kitchen tablet",
            "the name the registry announces is the name the user chose"
        );
    }

    #[test]
    fn a_blank_name_is_refused_and_the_old_one_is_kept() {
        let state = testing::state();
        let before = identity_get(&state).expect("an identity").display_name;
        let error = identity_set_display_name(&state, "   ").expect_err("a blank name");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(identity_get(&state).expect("an identity").display_name, before, "nothing changed");
    }

    #[test]
    fn a_name_with_a_control_character_in_it_is_refused_and_the_old_one_is_kept() {
        // A newline in a name is how a log line is forged and a carriage return is how a device-list row is rewritten.
        // The kernel refuses it; this reports the refusal rather than restating the rule.
        let state = testing::state();
        let error = identity_set_display_name(&state, "Study\nPC").expect_err("a name with a newline in it");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            identity_get(&state).expect("an identity").display_name,
            synra_core::DEFAULT_DISPLAY_NAME,
            "nothing changed"
        );
    }

    #[test]
    fn a_name_of_punctuation_is_a_name_the_kernel_accepts() {
        // ★ Asserting the permissive answer, on purpose.
        //
        // The first version of this test asserted that `***` was refused, because a name of nothing but punctuation
        // looks wrong. It is not refused — the kernel's rule is about control characters and length, not legibility —
        // and the test failing is what showed that the shell had been carrying a stricter rule of its own. A device
        // called `***` is unusual, not invalid, and refusing it would mean the shell and the CLI disagree.
        let state = testing::state();
        identity_set_display_name(&state, "***").expect("a name of punctuation");
        assert_eq!(identity_get(&state).expect("an identity").display_name, "***");
    }

    #[test]
    fn a_name_too_long_to_fit_a_device_card_is_refused_with_the_limit_in_the_message() {
        let state = testing::state();
        let too_long = "a".repeat(synra_core::MAX_DISPLAY_NAME_LEN + 1);
        let error = identity_set_display_name(&state, &too_long).expect_err("a name that cannot fit");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            error.params["maxChars"].as_u64(),
            Some(u64::try_from(synra_core::MAX_DISPLAY_NAME_LEN).unwrap_or(u64::MAX)),
            "the refusal tells the interface how long is too long"
        );
    }

    #[test]
    fn a_name_of_exactly_the_limit_is_accepted() {
        let state = testing::state();
        let at_limit = "a".repeat(synra_core::MAX_DISPLAY_NAME_LEN);
        identity_set_display_name(&state, &at_limit).expect("a name at the limit");
        assert_eq!(identity_get(&state).expect("an identity").display_name, at_limit);
    }

    #[test]
    fn a_state_can_be_built_without_a_plugin_host() {
        let state = testing::state();
        assert!(state.plugin_set().is_err(), "no engine means a refusal, not an empty set");
    }
}
