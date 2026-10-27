//! Actual desktop host adapter and component exports, without opening a window.

use super::*;
use std::time::{Duration, Instant};
use synra_core::dispatch::{PluginSet, PluginSpec};
use synra_plugin_host::{
    Backend, BackendKind, CallBudget, CallOutcome, InstancePolicy, Work, quota::Quota,
};
use synra_protocol::TraceId;
use synra_testkit::plugin_events::{DEVICE, EVENT, EventPlugin};

#[test]
fn desktop_guest_events_reach_a_receiver_with_authoritative_context() {
    let root = tempfile::tempdir().unwrap();
    let mut db = SynraDb::open(&root.path().join("events.sqlite3")).unwrap();
    db.migrate().unwrap();
    let emitter = EventPlugin::emitter(false).unwrap();
    let receiver = EventPlugin::receiver(None).unwrap();
    emitter.persist(&db).unwrap();
    receiver.persist(&db).unwrap();
    let db = Arc::new(Mutex::new(db));
    let host = Arc::new(DesktopHost::new(Arc::clone(&db)));
    let service = host.events();
    let engine =
        synra_plugin_host::engine::WasmtimeBackend::new(BackendKind::WasmtimeCranelift).unwrap();
    let set = PluginSet::new(Backend::new(Arc::new(engine)), host)
        .with_event_delivery(Arc::clone(&service))
        .into_shared()
        .unwrap();
    {
        let mut set = set.lock();
        for plugin in [&emitter, &receiver] {
            set.load_with_lifecycle(
                PluginSpec {
                    id: plugin.id.clone(),
                    version: "1.0.0".to_owned(),
                    wasm: plugin.wasm.clone(),
                    grants: CapabilitySet::parse_all(plugin.grants.iter().map(String::as_str))
                        .unwrap(),
                    policy: InstancePolicy::Stateful,
                    limit: 1,
                    quota: Quota::default(),
                },
                synra_plugin_api::manifest_digest(&plugin.manifest),
                false,
            )
            .unwrap();
            assert!(matches!(
                set.call(
                    &plugin.id,
                    &Work::Activate {
                        config_json: "{}".to_owned()
                    },
                    CallBudget::from_quota(&Quota::default()),
                    1
                )
                .unwrap(),
                CallOutcome::Ok(_)
            ));
            set.mark_active(&plugin.id).unwrap();
        }
        let trace = TraceId::new();
        assert!(matches!(
            set.call_traced(
                &emitter.id,
                &Work::InvokeAction {
                    name: "emit".to_owned(),
                    args_json: r#"{"text":"desktop🖱️"}"#.to_owned(),
                },
                CallBudget::from_quota(&Quota::default()),
                1,
                Some(trace)
            )
            .unwrap(),
            CallOutcome::Ok(_)
        ));
        drop(set);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while service.report().delivered == 0 {
        assert!(
            Instant::now() < deadline,
            "desktop event delivery timed out: {:?}",
            service.report()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let context: serde_json::Value = serde_json::from_str(
        &db.lock()
            .storage_get(receiver.id.as_str(), "received")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(context["event"], EVENT);
    assert_eq!(context["from"], DEVICE);
    assert_eq!(context["senderPluginId"], emitter.id.as_str());
    assert_eq!(context["payload"]["text"], "desktop🖱️");
    assert_eq!(service.report().refused, 0);
    service.stop();
    while service.report().worker_running {
        assert!(
            Instant::now() < deadline,
            "desktop event worker did not stop"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let stopped = Arc::downgrade(&service);
    drop(set);
    drop(service);
    drop(db);
    while stopped.strong_count() != 0 {
        assert!(
            Instant::now() < deadline,
            "desktop event worker retained its database"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    root.close().unwrap();
}

#[test]
fn callers_outside_a_guest_execution_cannot_spoof_event_emission() {
    let mut db = SynraDb::open_in_memory().unwrap();
    db.migrate().unwrap();
    let host = DesktopHost::new(Arc::new(Mutex::new(db)));
    let error = host
        .event_emit(&PluginId::parse("@acme/emitter").unwrap(), EVENT, "{}")
        .unwrap_err();
    assert_eq!(
        error.code,
        synra_protocol::ErrorCode::CapabilityDenied.as_str()
    );
}
