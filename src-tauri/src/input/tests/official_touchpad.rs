//! Opt-in production package → UI RPC → Wasm gestures → peer link → pointer acceptance.
use crate::{AppState, commands, input::Input};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use synra_platform::pointer::{Command, Pointer};
use synra_plugin_host::{
    Backend,
    engine::{BackendKind, WasmtimeBackend},
};

const PLUGIN: &str = "@synra-plugin/touchpad";
const GRANTS: [&str; 5] = [
    "device:list",
    "device:input",
    "device:send:*",
    "platform:control-pointer",
    "action:declare:touchpad",
];

#[derive(Default)]
struct Effects {
    commands: Vec<Command>,
    created: usize,
    released: usize,
}
struct Probe {
    effects: Arc<Mutex<Effects>>,
    native: Option<Box<dyn Pointer>>,
}
impl Pointer for Probe {
    fn apply(&mut self, command: Command) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            native.apply(command)?;
        }
        self.effects.lock().commands.push(command);
        Ok(())
    }
    fn release(&mut self) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            native.release()?;
        }
        self.effects.lock().released += 1;
        Ok(())
    }
}

fn installed(effects: Option<Arc<Mutex<Effects>>>) -> (crate::testing::Root, AppState) {
    let package = PathBuf::from(
        std::env::var_os("SYNRA_TOUCHPAD_PACKAGE")
            .expect("set SYNRA_TOUCHPAD_PACKAGE to CLI-built dist"),
    );
    let root = crate::testing::Root::fresh("touchpad-package");
    synra_store::Paths::under(root.path())
        .ensure_directories()
        .unwrap();
    let mut state = crate::testing::state_in(&root);
    if let Some(effects) = effects {
        state.input = Input::with_pointer(
            Arc::clone(&state.db),
            &state.host,
            Arc::clone(&state.events),
            Arc::new(move || {
                // Explicit local acceptance only: run against an owned pointer-event probe window.
                // Ordinary test runs never inject native effects.
                let native = if std::env::var("SYNRA_NATIVE_POINTER_SMOKE").as_deref()
                    == Ok("owned-probe")
                {
                    Some(synra_platform::pointer::native()?)
                } else {
                    None
                };
                effects.lock().created += 1;
                Ok(Box::new(Probe {
                    effects: Arc::clone(&effects),
                    native,
                }))
            }),
        );
    }
    let host = crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    )
    .with_input_controller(Arc::clone(&state.input.controller));
    let backend = WasmtimeBackend::new(BackendKind::WasmtimeCranelift).unwrap();
    let state = state.with_plugins(Arc::new(Mutex::new(synra_core::dispatch::PluginSet::new(
        Backend::new(Arc::new(backend)),
        Arc::new(host),
    ))));
    let archive = root.path().join("touchpad.synra");
    synra_core::install::package_directory(&package, &archive).unwrap();
    let installed = commands::plugins::plugins_install(
        &state,
        &synra_ipc::InstallSourceDto::Path {
            path: archive.to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    assert_eq!(installed.plugin_id.as_str(), PLUGIN);
    commands::plugins::plugins_consent_respond(&state, PLUGIN, &GRANTS.map(str::to_owned)).unwrap();
    commands::plugins::plugins_enable(&state, PLUGIN).unwrap();
    (root, state)
}

fn pair(state: &AppState, peer: &AppState, address: Option<SocketAddr>) {
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: peer.host.lock().device_id().to_string(),
            display_name: "Test peer".to_owned(),
            platform: "desktop".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1_000,
            address: address.map(|a| a.to_string()),
        })
        .unwrap();
}

fn invoke(state: &AppState, token: &str, args: Value) -> Value {
    commands::plugin_ui::plugin_ui_rpc(
        state,
        token,
        "self.invoke",
        &json!({"action":"touchpad","args":args}),
    )
    .unwrap()
}

fn frame(state: &AppState, token: &str, time: u64, contacts: Value) {
    invoke(
        state,
        token,
        json!({"op":"frame","nowMs":time,"contacts":contacts}),
    );
}

fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "bounded input acceptance timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn official_touchpad_controls_only_after_host_consent_and_releases_on_stop() {
    let effects = Arc::new(Mutex::new(Effects::default()));
    let (_pc_root, pc) = installed(Some(Arc::clone(&effects)));
    let pc = Arc::new(pc);
    let (_phone_root, phone) = installed(None);
    let address = pc
        .host
        .lock()
        .listen_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .unwrap();
    pc.host.lock().listen_nonblocking().unwrap();
    pair(&phone, &pc, Some(address));
    pair(&pc, &phone, None);
    let server = Arc::clone(&pc);
    let incoming = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            if let Some(stream) = server.host.lock().try_accept().unwrap() {
                break stream;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        let (dial, flags) = server.host.lock().dialling_parts();
        let now = synra_core::reading_millis(synra_store::sqlite::now_ms());
        let accepted = dial.accept_stream(stream, flags, now).unwrap();
        let mut link = server.host.lock().adopt(accepted, now).unwrap();
        link.set_read_timeout(Some(Duration::from_secs(3)));
        let request = link.receive().unwrap();
        assert!(synra_core::input::serve_if_requested(
            &server.host,
            &mut link,
            &request
        ));
    });
    let opened = match commands::plugin_ui::open_request(&phone, PLUGIN).unwrap() {
        commands::plugin_ui::OpenDecision::Open(request) => request,
        commands::plugin_ui::OpenDecision::AlreadyOpen { .. } => panic!("fresh page"),
    };
    commands::plugin_ui::plugin_ui_ready(&phone, &opened.label).unwrap();
    let token = phone
        .hosted_ui
        .lock()
        .get(&opened.label)
        .unwrap()
        .nonce
        .to_string();
    let target = pc.host.lock().device_id();
    assert_eq!(
        invoke(&phone, &token, json!({"op":"connect","target":target}))["phase"],
        "pending"
    );
    wait(|| pc.input.snapshot().session.is_some());
    let pending = pc.input.snapshot().session.unwrap();
    assert_eq!(
        effects.lock().created,
        0,
        "pairing and grants alone cannot create native input"
    );
    commands::input::devices_input_respond(&pc, &pending.session_id.to_string(), true).unwrap();
    wait(|| invoke(&phone, &token, json!({"op":"status"}))["phase"] == "active");
    frame(&phone, &token, 0, json!([{"id":1,"x":0,"y":0}]));
    frame(&phone, &token, 20, json!([{"id":1,"x":12,"y":-4}]));
    frame(&phone, &token, 30, json!([]));
    frame(&phone, &token, 500, json!([{"id":2,"x":0,"y":0}]));
    frame(&phone, &token, 520, json!([]));
    frame(&phone, &token, 900, json!([{"id":3,"x":0,"y":0}]));
    frame(
        &phone,
        &token,
        910,
        json!([{"id":3,"x":0,"y":0},{"id":4,"x":20,"y":0}]),
    );
    frame(&phone, &token, 920, json!([{"id":4,"x":20,"y":0}]));
    frame(&phone, &token, 930, json!([]));
    frame(
        &phone,
        &token,
        1400,
        json!([{"id":5,"x":0,"y":0},{"id":6,"x":20,"y":0}]),
    );
    frame(
        &phone,
        &token,
        1420,
        json!([{"id":5,"x":0,"y":80},{"id":6,"x":20,"y":80}]),
    );
    frame(&phone, &token, 1430, json!([]));
    frame(&phone, &token, 2000, json!([{"id":7,"x":0,"y":0}]));
    frame(&phone, &token, 2020, json!([]));
    frame(&phone, &token, 2080, json!([{"id":8,"x":0,"y":0}]));
    frame(&phone, &token, 2200, json!([{"id":8,"x":0,"y":0}]));
    frame(&phone, &token, 2220, json!([{"id":8,"x":12,"y":0}]));
    frame(&phone, &token, 2240, json!([]));
    wait(|| {
        invoke(&phone, &token, json!({"op":"status"}));
        effects.lock().commands.len() >= 8
    });
    assert_eq!(
        effects.lock().commands,
        [
            Command::Move { dx: 12.0, dy: -4.0 },
            Command::LeftClick,
            Command::RightClick,
            Command::Scroll { dx: 0.0, dy: 80.0 },
            Command::LeftClick,
            Command::LeftDown,
            Command::Move { dx: 12.0, dy: 0.0 },
            Command::LeftUp
        ]
    );
    // Leave a second drag held: host Stop, rather than a final phone up, must release it.
    frame(&phone, &token, 3000, json!([{"id":9,"x":0,"y":0}]));
    frame(&phone, &token, 3020, json!([]));
    frame(&phone, &token, 3080, json!([{"id":10,"x":0,"y":0}]));
    frame(&phone, &token, 3200, json!([{"id":10,"x":0,"y":0}]));
    wait(|| {
        invoke(&phone, &token, json!({"op":"status"}));
        effects.lock().commands.len() >= 10
    });
    commands::input::devices_input_stop(&pc, &pending.session_id.to_string()).unwrap();
    incoming.join().unwrap();
    assert_eq!(effects.lock().released, 1);
    wait(|| invoke(&phone, &token, json!({"op":"status"}))["phase"] == "ended");
    commands::plugin_ui::forget_window(&phone, &opened.label);
    if let Some(destination) = std::env::var_os("SYNRA_TOUCHPAD_ARCHIVE") {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .unwrap();
        let mut source = std::fs::File::open(_pc_root.path().join("touchpad.synra")).unwrap();
        std::io::copy(&mut source, &mut output).unwrap();
    }
}
