use std::sync::Arc;

use crate::state::AppState;

/// The window's discovery worker, or a window without one.
///
/// # Errors
///
/// None, and for the same reason [`with_plugins_or_refuse`] has none: a machine with no network, a firewall and a sandbox
/// all produce the same bind failure, and none of them is a reason to lose the window — this device simply cannot be
/// *found*, and every other command still works.
///
/// ★ What it costs is the **scan**, and what that looks like to a user is worth writing down: `devices_scan_start` refuses
/// a network scan with the reason, and the interface falls back to the cached read — the devices already known are still
/// listed. A window that refused to open over a busy UDP port would trade the user's whole application for one button.
pub(super) fn with_discovery_or_report(state: AppState) -> AppState {
    // ★ 广播的是**对端该拨的端口**：窗口监听的随机端口，不是发现端口。
    // 曾经写的是 `DISCOVERY_PORT`，于是每台设备都被记录成一个「能发现、拨不通」的地址——
    // 对面把 TCP 连到 UDP 发现端口上，配对永远停在「联系不上」。
    let Some(port) = state.listener.as_ref().map(|(_, port)| *port) else {
        eprintln!("synra: pairing is unavailable: no transport port to announce");
        return state;
    };
    let announcement = state.announcement(port);
    // The announcement the worker makes: it reports an identifier, and the host builds the card and publishes it.
    let announce: crate::discovery::Announce = {
        let db = Arc::clone(&state.db);
        let host = state.host.clone();
        let events = Arc::clone(&state.events);
        Arc::new(move |device_id: &str| {
            if let Ok(Some(card)) = crate::commands::devices::card_for(&db, &host, device_id) {
                events.emit(synra_ipc::event::KernelEvent::DeviceCardUpdated(Box::new(
                    card,
                )));
            }
        })
    };
    let started = crate::discovery::Discovery::start(
        announcement,
        synra_protocol::limits::DISCOVERY_PORT,
        Arc::clone(&state.db),
        state.host.clone(),
        state.started_at_ms,
        announce,
    );

    match started {
        Ok(discovery) => {
            match discovery.responder_address() {
                Ok(address) => println!("synra: discovery is listening on {address}"),
                Err(error) => eprintln!("synra: {error}"),
            }
            state.with_discovery(discovery)
        }
        Err(reason) => {
            eprintln!("synra: discovery is not running: {reason}");
            state
        }
    }
}
