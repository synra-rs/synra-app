use std::sync::Arc;

use synra_core::dispatch::{PluginSet, PluginSpec};
use synra_ipc::event::{PluginCustomEvent, PluginEvent};
use synra_plugin_host::{
    Backend, BackendKind, CallBudget, CallOutcome, InstancePolicy, Work, quota::Quota,
};
use synra_protocol::event::EventName;
use synra_testkit::plugin_events::{EVENT, EventPlugin};

use super::super::projection::project_custom;
use super::{DEADLINE, ErrorCode, json, page, pending, rpc, subscribe};
use crate::commands::plugin_ui::bind_custom_events;
use crate::plugin_host::DesktopHost;

#[tokio::test]
async fn an_admitted_guest_event_reaches_only_a_granted_page() {
    let (_root, state) = crate::testing::file_state();
    let emitter = EventPlugin::emitter(false).unwrap();
    emitter.persist(&state.db()).unwrap();
    let host = Arc::new(DesktopHost::new(Arc::clone(&state.db)));
    let service = host.events();
    let engine =
        synra_plugin_host::engine::WasmtimeBackend::new(BackendKind::WasmtimeCranelift).unwrap();
    let set = PluginSet::new(Backend::new(Arc::new(engine)), host)
        .with_event_delivery(Arc::clone(&service))
        .into_shared()
        .unwrap();
    let state = Arc::new(state.with_plugins(Arc::clone(&set)));
    bind_custom_events(&state);

    page(
        &state,
        "reader",
        10,
        "@test/reader",
        &["event:subscribe:_plugin.@acme/emitter.*"],
    );
    page(
        &state,
        "other",
        11,
        "@test/other",
        &["event:subscribe:_plugin.@acme/other.*"],
    );
    let reader = subscribe(&state, "reader", 10, EVENT, 1).await;
    let refused = rpc(
        &state,
        "other",
        11,
        "event.subscribe",
        json!({"subscriptionId":1,"pattern":EVENT}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    let other = subscribe(&state, "other", 11, "_plugin.*.*", 1).await;
    let mut foreign = Box::pin(rpc(
        &state,
        "other",
        11,
        "event.poll",
        json!({"streamId":other}),
    ));
    pending(foreign.as_mut()).await;

    let quota = Quota::default();
    {
        let mut plugins = set.lock();
        plugins
            .load_with_lifecycle(
                PluginSpec {
                    id: emitter.id.clone(),
                    version: "1.0.0".to_owned(),
                    wasm: emitter.wasm,
                    grants: synra_capability::CapabilitySet::parse_all(
                        emitter.grants.iter().map(String::as_str),
                    )
                    .unwrap(),
                    policy: InstancePolicy::Stateful,
                    limit: 1,
                    quota: quota.clone(),
                },
                synra_plugin_api::manifest_digest(&emitter.manifest),
                false,
            )
            .unwrap();
        let budget = CallBudget::from_quota(&quota);
        assert!(matches!(
            plugins
                .call(
                    &emitter.id,
                    &Work::Activate {
                        config_json: "{}".to_owned()
                    },
                    budget,
                    1
                )
                .unwrap(),
            CallOutcome::Ok(_)
        ));
        plugins.mark_active(&emitter.id).unwrap();
        assert!(matches!(
            plugins
                .call(
                    &emitter.id,
                    &Work::InvokeAction {
                        name: "emit".to_owned(),
                        args_json: r#"{"text":"hello🖱️"}"#.to_owned(),
                    },
                    budget,
                    1
                )
                .unwrap(),
            CallOutcome::Ok(_)
        ));
    }

    let frame = tokio::time::timeout(
        DEADLINE,
        rpc(
            &state,
            "reader",
            10,
            "event.poll",
            json!({"streamId":reader}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(frame["name"], EVENT);
    assert_eq!(frame["payload"], json!({"text":"hello🖱️"}));
    assert_eq!(frame.as_object().unwrap().len(), 4);
    pending(foreign.as_mut()).await;
    service.stop();
}

#[tokio::test]
async fn recovery_identifies_only_the_subscriptions_with_lost_custom_events() {
    let (_root, state) = crate::testing::file_state();
    page(
        &state,
        "reader",
        10,
        "@test/reader",
        &["event:subscribe:_plugin.@acme/emitter.*", "device:list"],
    );
    let stream = subscribe(&state, "reader", 10, EVENT, 1).await;
    subscribe(&state, "reader", 10, "_plugin.@acme/emitter.other", 2).await;
    subscribe(&state, "reader", 10, "device.*", 3).await;
    let event = PluginEvent::Custom(
        PluginCustomEvent::new(EventName::parse(EVENT).unwrap(), Default::default()).unwrap(),
    );
    project_custom(&state, event.clone(), 1);
    let poll = || {
        rpc(
            &state,
            "reader",
            10,
            "event.poll",
            json!({"streamId":stream}),
        )
    };
    assert_eq!(poll().await.unwrap()["seq"], 1);
    for at in 2..=10 {
        project_custom(&state, event.clone(), at);
    }
    assert_eq!(poll().await.unwrap()["seq"], 3);
    let recover = |after| {
        rpc(
            &state,
            "reader",
            10,
            "event.resync",
            json!({"streamId":stream,"afterSeq":after}),
        )
    };
    let snapshot = recover(1).await.unwrap();
    assert_eq!(snapshot["seq"], 10);
    assert_eq!(
        snapshot["events"],
        json!([{"name":"device.changed","payload":{"devices":[]}}])
    );
    assert_eq!(snapshot["unreplayedSubscriptionIds"], json!([1]));
    assert!(
        recover(10)
            .await
            .unwrap()
            .get("unreplayedSubscriptionIds")
            .is_none()
    );
    assert_eq!(
        recover(11).await.unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    project_custom(&state, event, 11);
    assert_eq!(poll().await.unwrap()["seq"], 11);
}
