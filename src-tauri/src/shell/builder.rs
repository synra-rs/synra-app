use tauri::Manager;

use crate::{commands, state::AppState};

use super::{package::open_package, runtime::Background, startup::app_state, wrappers};

/// The one global shortcut: summon the window when it is hidden behind other applications.
///
/// `CommandOrControl` rather than `Ctrl`: on macOS the conventional summon key is Command, and a shortcut that only works
/// on two of the three desktops is the kind of platform detail a user discovers by it not working.
#[cfg(any(desktop, test))]
pub(super) const WAKE_SHORTCUT: &str = "CommandOrControl+Shift+Space";

/// The shell, with the state it reads and the commands it answers, and no window opened yet.
///
/// Split out of [`run_window`] so the two platforms can differ in how they *enter* the window — a desktop process
/// returns an exit code, an Android activity calls in through JNI and has nowhere to return one to — while sharing how
/// the window is built.
pub fn builder() -> tauri::Builder<tauri::Wry> {
    let builder = tauri::Builder::default()
        // ★ The one native dialog this application needs: choosing a file to send. It is a plugin rather than a
        // dependency of the webview, and its permission is named in `capabilities/default.json` beside `core:default`.
        .plugin(tauri_plugin_dialog::init())
        // ★ `synra://` links. The plugin owns the scheme registration and the event; `deeplink.rs` owns what a link
        // means, and the setup below only hands it the URLs.
        .plugin(tauri_plugin_deep_link::init());
    // ★ The two that only a desktop has, added after the chain is built because `.plugin()` returns a builder and Rust
    // cannot `#[cfg]` a single method call inside a chain. A phone has no tray to put an icon in and no session to start
    // with, and neither plugin has an Android implementation at all — see `Cargo.toml`'s note.
    #[cfg(desktop)]
    let builder = builder
        // ★ Registering the application to start with the session is the platform's job; the settings page only asks
        // the plugin to switch it on or off. Starting with the arguments we were launched with, not a second copy.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // ★ One shortcut, and it does the thing a tray app needs: bring the window back without hunting for it. The
        // handler is here rather than in a page because a page is not running when the window is hidden.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        crate::window::show_main(app);
                    }
                })
                .build(),
        );
    builder
        // ★ A plugin page is served from its own origin, and this is the only place that knows how: the address is split
        // against the ids the host actually opened (`hosted_ui`), and the file comes from that plugin's bundle through the
        // same path rules `ui_file` tests. Everything a request cannot have — another plugin's files, `..`, a missing
        // bundle — is a refusal here rather than a guess.
        .register_uri_scheme_protocol("synra-plugin", |ctx, request| {
            crate::commands::plugin_ui::serve(ctx.app_handle(), ctx.webview_label(), &request)
        })
        .setup(|app| {
            #[cfg(all(feature = "official-plugin-tests", debug_assertions, not(mobile)))]
            let smoke_package = official_smoke::package(app.handle())?;
            // ★ The single-instance check, **before** the state opens the database or binds a listener. A second launch
            // has nothing to do here: it has already written the file that asks the running instance to come forward.
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("the data directory could not be found: {error}"))?;
            // ★ A phone has no console, so from here on every step is named in a file. See `crash`.
            crate::crash::install(&data_dir);
            crate::crash::note("setup: data directory known");
            // ★ Desktop only, and this is a platform fact rather than a preference.
            //
            // The lock exists for the second launch of a **window**: it asks the first one to come forward, and on a
            // desk that is the difference between one tray icon and two. A phone has no second window to bring
            // forward — the system hands the application to the foreground and no new process starts — so the only
            // thing this check can do on Android is decide, wrongly, that the process is a duplicate and exit.
            #[cfg(desktop)]
            let mut instance_request = None;
            #[cfg(mobile)]
            let instance_request: Option<std::path::PathBuf> = None;
            #[cfg(desktop)]
            match crate::instance::claim(&data_dir) {
                Ok(crate::instance::Start::First(guard)) => {
                    instance_request = Some(guard.request_path().to_path_buf());
                    // Managed, not dropped: the operating-system lock lives as long as this value does, and this value
                    // lives as long as the application.
                    app.manage(guard);
                }
                Ok(crate::instance::Start::Second) => {
                    // ★ The running instance has been asked to show itself; this process has nothing to do. Returning
                    // before `app_state` is the point: a second database and a second listener port must not exist.
                    app.handle().exit(0);
                    return Ok(());
                }
                Err(error) => {
                    // ★ A lock we cannot take is not a reason to refuse to run. A read-only data directory would
                    // otherwise make the window unopenable, and two instances is the lesser fault.
                    eprintln!("synra: the single-instance lock could not be taken: {error}");
                }
            }
            crate::crash::note("setup: opening the state");
            let state = app_state(app.handle())?;
            crate::crash::note("setup: state ready");
            // ★ The event bridge, started before the state is moved into the window. It holds a clone of the bus, so a
            // command that publishes later finds a subscriber waiting — a bridge started after the first command would
            // miss exactly the event that command raised.
            let bridge_cancel = synra_core::supervisor::Cancellation::new();
            crate::events::spawn(
                app.handle().clone(),
                std::sync::Arc::clone(&state.events),
                &bridge_cancel,
            );
            let state = std::sync::Arc::new(state);
            {
                // Pairing changes call back here, so the window learns about a request without polling.
                let weak = std::sync::Arc::downgrade(&state);
                state
                    .host
                    .lock()
                    .pairing()
                    .set_sink(std::sync::Arc::new(move || {
                        let Some(state) = weak.upgrade() else { return };
                        let snapshot = commands::pairing::snapshot_of(&state.host.lock().pairing());
                        state.publish(synra_ipc::event::KernelEvent::PairingUpdated(Box::new(
                            snapshot,
                        )));
                    }));
            }
            app.manage(state);
            #[cfg(all(feature = "official-plugin-tests", debug_assertions, not(mobile)))]
            if let Some(package) = smoke_package {
                official_smoke::run(app.handle(), &package)?;
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt as _;
                let handle = app.handle().clone();
                let _ = app
                    .deep_link()
                    .on_open_url(move |event| crate::deeplink::receive(&handle, event.urls()));
                // ★ A cold start that *is* the link: the event may have been emitted before this listener existed, and
                // `get_current` is the plugin's answer for that window.
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    crate::deeplink::receive(app.handle(), urls);
                }
                #[cfg(any(windows, target_os = "linux"))]
                if let Err(error) = app.deep_link().register_all() {
                    eprintln!("synra: the synra:// scheme could not be registered: {error}");
                }
            }
            // ★ A `.synra` file opened from the desktop. The opener is shared with drag-and-drop and the macOS "open
            // document" event, so all three entry points decide install/update the same way.
            if let Some(package) = crate::launch::package_from_args(std::env::args()) {
                open_package(app.handle(), &package);
            }
            // ★ Dragging a package onto the window is the same gesture as double-clicking it. Other files are ignored:
            // this shell has nowhere to send them yet, and pretending otherwise would be a drop target that does nothing.
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop {
                        paths, ..
                    }) = event
                    {
                        let mut opened = false;
                        for path in paths {
                            if path
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("synra"))
                            {
                                open_package(&handle, path.as_path());
                                opened = true;
                            }
                        }
                        // ★ A drop that installed nothing is not a successful gesture. Other files have no destination
                        // yet, so say that rather than leaving the window looking like it ignored the user.
                        if !opened && !paths.is_empty() {
                            let state = handle.state::<std::sync::Arc<AppState>>();
                            state.publish(synra_ipc::event::KernelEvent::Notice {
                                message: "notice.dropIgnored".to_owned(),
                                params: serde_json::json!({}),
                            });
                        }
                    }
                });
            }
            // ★ A shortcut another application already owns is a missing convenience, not a reason to lose the window —
            // so the failure is reported and the rest of setup continues.
            //
            // Desktop only: there is no global shortcut on Android, and the plugin is not in the dependency graph there.
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
                if let Err(error) = app.global_shortcut().register(WAKE_SHORTCUT) {
                    eprintln!("synra: the global shortcut could not be registered: {error}");
                }
            }
            // The bridge and the pairing watcher are the two threads that outlive the window but own no socket; their
            // tokens go in one value so the shutdown does not have to remember a second place. The retry loop's token
            // belongs in the same list: it sleeps between attempts, and nothing else would tell it the process is leaving.
            let devices_pairing_cancel = synra_core::supervisor::Cancellation::new();
            let resume_cancel = synra_core::supervisor::Cancellation::new();
            let updates_cancel = synra_core::supervisor::Cancellation::new();
            app.manage(Background {
                cancels: vec![
                    bridge_cancel,
                    devices_pairing_cancel.clone(),
                    resume_cancel.clone(),
                    updates_cancel.clone(),
                ],
            });
            // ★ The listener starts after the state is managed, because it reads that state to build its host and router. The token
            // is managed rather than dropped so a later shutdown can stop the loop: without it the thread lives until the process
            // does, and a restart would fail to bind a port the old one still holds.
            crate::crash::note("setup: opening the listener");
            app.manage(crate::listener::spawn(app.handle()));
            crate::crash::note("setup: listener ready");
            // ★ A dispatch that was in flight when the last process ended is sent again with the same request identifier —
            // the receiving side deduplicates on it, so this either runs the action once or returns the outcome that already
            // happened. The immediate pass runs first, then a bounded backoff keeps trying what is still in flight; the
            // window does not wait for any of it.
            {
                let state = app.state::<std::sync::Arc<AppState>>().inner().clone();
                let token = resume_cancel.clone();
                let spawned = std::thread::Builder::new()
                    .name("synra-intent-resume".to_owned())
                    .spawn(move || {
                        crate::commands::intent_resume::run(
                            &state,
                            &token,
                            crate::commands::intent_resume::RetryPolicy::default(),
                        );
                    });
                if let Err(error) = spawned {
                    eprintln!("synra: the intent resume thread could not start: {error}");
                }
            }
            // ★ Once a day, ask the registry whether anything installed has a newer version, and put one notice on the
            // page if so. It never installs: the design's automatic case needs trust and capability checks this pass does
            // not make, and a check that installed silently would be a decision nobody took. The user can turn it off on
            // the settings page (`plugins.auto_update`).
            {
                let state = app.state::<std::sync::Arc<AppState>>().inner().clone();
                let token = updates_cancel.clone();
                let spawned = std::thread::Builder::new()
                    .name("synra-plugin-updates".to_owned())
                    .spawn(move || crate::commands::plugin_updates::run(&state, &token));
                if let Err(error) = spawned {
                    eprintln!("synra: the plugin update check could not start: {error}");
                }
            }
            // ★ The tray, and it is reported rather than fatal: the window is the application, and a desktop without an
            // indicator host (some Linux setups) must still get one. What it buys is the second way back to a window and
            // a **deliberate** way out — see `run_window`, where closing the window hides it instead of ending the process.
            //
            // Desktop only: `tauri::tray` does not exist on Android, and the module is gated the same way.
            #[cfg(desktop)]
            if let Err(error) = crate::tray::install(app.handle()) {
                eprintln!("synra: the tray icon could not be installed: {error}");
            }
            // ★ Last, because a request honoured before there is a window would find nothing to show.
            if let Some(request) = instance_request {
                crate::instance::watch(app.handle().clone(), request);
            }
            // ★ The window starts hidden (`visible: false` in `tauri.conf.json`) and is shown here, after the
            // single-instance check. A second launch would otherwise create and flash a window before deciding it has
            // nothing to do.
            // ★ One function for "bring the window forward", shared with the tray. A second launch is the third caller,
            // and three copies of show/unminimise/focus is how one of them ends up missing a step.
            //
            // Desktop only: on Android the activity is already in front — there is no hidden window, and
            // `window::show_main` is not compiled there.
            #[cfg(desktop)]
            crate::window::show_main(app.handle());
            Ok(())
        })
        .invoke_handler(wrappers::handler())
}
