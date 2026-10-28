//! Document admission and authority races use real store records and the production entry points.

use super::*;

#[test]
fn concurrent_opens_share_one_live_nonce_and_the_same_window() {
    let (_root, state) = installed_with_ui();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    super::super::open_request(&state, ID).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut labels = Vec::new();
    let mut created = 0;
    for result in results {
        labels.push(match result {
            OpenDecision::Open(request) => {
                created += 1;
                request.label
            }
            OpenDecision::AlreadyOpen { label } => label,
        });
    }
    assert_eq!(created, 1);
    assert_eq!(labels.first(), labels.last());
    assert_eq!(state.ui_sessions.issued(), 1);
    let label = labels.first().unwrap();
    assert!(super::super::plugin_ui_ready(&state, label).is_ok());
}

#[test]
fn disabled_or_untrusted_plugins_cannot_open_pages() {
    let (_root, state) = installed_with_ui();
    state.db().set_plugin_enabled(ID, false, 1).unwrap();
    assert_eq!(
        super::super::open_request(&state, ID).unwrap_err().code,
        ErrorCode::CapabilityDenied
    );
    state.db().set_plugin_enabled(ID, true, 1).unwrap();
    state.db().set_plugin_trusted(ID, false, 1).unwrap();
    assert_eq!(
        super::super::open_request(&state, ID).unwrap_err().code,
        ErrorCode::CapabilityDenied
    );
    assert!(state.ui_sessions.is_empty());
}

#[test]
fn authority_probes_and_resource_reads_do_not_mark_the_page_ready() {
    let (_root, state) = installed_with_ui();
    let request = opened(&state, ID);
    let path = state.paths.plugin_dir(ID).join("ui/dist");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("index.html"), "<!doctype html>").unwrap();
    assert!(super::super::page_resource(&state, &request.label, &request.url).is_ok());
    assert!(super::super::stale_windows(&state).is_empty());
    assert_eq!(state.ui_sessions.report().ready, 0);
    super::super::plugin_ui_ready(&state, &request.label).unwrap();
    assert_eq!(state.ui_sessions.report().ready, 1);
}

#[test]
fn stale_windows_remain_pending_until_native_destruction_succeeds() {
    let (_root, state) = installed_with_ui();
    let request = opened(&state, ID);
    state.db().set_plugin_enabled(ID, false, 1).unwrap();
    for _ in 0..2 {
        assert!(super::super::stale_windows(&state).contains(&request.label));
        assert!(super::super::plugin_ui_ready(&state, &request.label).is_err());
    }
    state.db().set_plugin_enabled(ID, true, 1).unwrap();
    let new = opened(&state, ID);
    assert_ne!(new.label, request.label);
    super::super::forget_window(&state, &request.label);
    assert!(super::super::plugin_ui_ready(&state, &new.label).is_ok());
    assert!(super::super::stale_windows(&state).is_empty());
}

#[test]
fn an_expired_or_revoked_nonce_is_replaced_even_when_the_authority_did_not_change() {
    let (_root, state) = installed_with_ui();
    let first = opened(&state, ID);
    let nonce = state.hosted_ui.lock().get(&first.label).unwrap().nonce;
    state.ui_sessions.revoke(nonce);
    let second = opened(&state, ID);
    assert_ne!(first.label, second.label);
    assert!(second.superseded.contains(&first.label));
    assert!(super::super::plugin_ui_ready(&state, &first.label).is_err());
    assert!(super::super::plugin_ui_ready(&state, &second.label).is_ok());
}
