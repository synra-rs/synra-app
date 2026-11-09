use super::builder::builder;
#[cfg(target_os = "macos")]
use super::package::open_package;
use crate::state::AppState;
use tauri::Manager;

/// Builds and runs the window, reporting what stopped it rather than unwinding.
///
/// # Why closing the window hides it
///
/// Because this shell is a kernel: a listener on a port, transfers in flight, a plugin set loaded. Closing a window is how
/// a desktop application is put away, not how one is ended, and the two are not the same gesture for a program that other
/// devices can reach. The tray's **Quit** is the deliberate exit; the close button asks the window to go away.
///
/// # Errors
///
/// A message saying what stopped it.
pub fn run_window() -> Result<(), String> {
    let app = builder()
        .build(tauri::generate_context!())
        .map_err(|error| format!("the window could not be opened: {error}"))?;
    app.run(|app, event| {
        // ★ Every deliberate exit passes here, including the tray's Quit: asking the background workers to stop before
        // the process ends is what closes the listener's port and lets the current discovery turn finish its write.
        if let tauri::RunEvent::ExitRequested { .. } = &event {
            stop_background(app);
        }
        // ★ macOS delivers an opened file as an Apple Event, not as an argv entry. The same opener serves it; `synra://`
        // links arrive here too, so they go through the deep-link receiver the plugin uses.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &event {
            for url in urls {
                if url.scheme() == "file" {
                    if let Ok(path) = url.to_file_path()
                        && path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("synra"))
                    {
                        open_package(app, &path);
                    }
                } else {
                    crate::deeplink::receive(app, vec![url.clone()]);
                }
            }
        }
        if let tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } = event
            && label == "main"
        {
            // ★ Before the hide, not after: the request is the window's decision, and hiding a window whose close was
            // not prevented would be hiding one that is already going.
            api.prevent_close();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    });
    Ok(())
}

/// Asks every worker that outlives the window to stop.
///
/// # Why one function
///
/// Because a shutdown that lists its workers by hand is one that forgets the next worker somebody starts. The listener
/// and discovery are the two that own a socket; both are asked here, and a new one belongs in this list with them.
/// The stop tokens for the threads that outlive the window but own no socket.
///
/// The event bridge and the pairing watcher both live for the process; they are not the listener, whose token is managed
/// separately because it is created after this value, and not discovery, whose token lives in `AppState`.
pub(super) struct Background {
    pub(super) cancels: Vec<synra_core::supervisor::Cancellation>,
}

fn stop_background(app: &tauri::AppHandle) {
    // The listener's token is managed rather than held by `AppState`, because it is created after the state is managed.
    if let Some(managed) = app.try_state::<Option<synra_core::supervisor::Cancellation>>()
        && let Some(cancel) = managed.inner().as_ref()
    {
        cancel.cancel();
    }
    if let Some(background) = app.try_state::<Background>() {
        for cancel in &background.cancels {
            cancel.cancel();
        }
    }
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        if !state.input.shutdown() {
            eprintln!("synra: the pointer did not finish releasing before the shutdown deadline");
        }
        if let Some(discovery) = state.discovery.as_ref() {
            discovery.stop();
        }
    }
}

/// The entry point Android calls, and only there.
///
/// ★ **Why this is `#[cfg(mobile)]` rather than `#[cfg_attr(mobile, …)]`.**
///
/// With `cfg_attr` the attribute is skipped on a desktop build and what is left is an ordinary `pub fn`
/// that nothing calls — a function that exists so that a reader can see it and wonder. Gated on `mobile`,
/// it is **absent** from a desktop build, which is the honest description: a desktop process has `main`,
/// and this symbol is what a shared library offers instead of one.
///
/// The refusal is printed rather than returned because there is no caller to return it to: a mobile entry
/// point returns no exit code, and a panic across the FFI boundary would abort the process with nothing in
/// the log.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    if let Err(reason) = run_window() {
        // ★ A refusal here is the one failure a phone shows as "it closed by itself": the entry point returns no code
        // and nothing is listening on stderr, so the sentence is written where a person can fetch it.
        crate::crash::note(&format!("run_window refused: {reason}"));
        eprintln!("synra: {reason}");
    }
}
