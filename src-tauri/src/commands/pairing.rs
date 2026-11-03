//! 配对：把内核那张桌子翻译成界面能画的东西。
//!
//! # 为什么 `devices_pair` 立刻返回而不是等结果
//!
//! 因为它要给出的第一样东西是**码**，而码在拨号之前就生成了。等两分钟再回答，界面会在这两分钟里
//! 一片空白——而用户此时正需要看见一个数字去和对面核对。拨号与等待因此放在后台线程，
//! 结果通过 `PairingUpdated` 事件回来。
//!
//! # 为什么地址现在才查
//!
//! `DeviceCard` 对未配对的设备**不给地址**：那是它被发现时的地址，不是它现在的地址。
//! 真正要拨号时向内核问「现在往哪拨」，拿不到就如实说连不上，而不是拿一个旧地址碰运气。

use std::sync::Arc;

use synra_core::pairing::Service;
use synra_ipc::{
    IpcError, PairingAsk, PairingOffer, PairingOutcome, PairingPhase, PairingSnapshot,
};
use synra_protocol::DeviceId;

use crate::state::{AppState, internal};

/// 当前的配对状态，供界面渲染。
pub fn devices_pairing_snapshot(state: &AppState) -> Result<PairingSnapshot, IpcError> {
    Ok(snapshot_of(&state.host.lock().pairing()))
}

/// 发起配对：生成码、开始拨号，立刻回答「等待」。
///
/// # Errors
///
/// [`IpcError`] 当这台设备不在记录里，或此刻没有可达的地址。
pub fn devices_pair(state: &Arc<AppState>, device_id: &str) -> Result<PairingSnapshot, IpcError> {
    let peer = device_id
        .parse::<DeviceId>()
        .map_err(|error| internal(format!("{device_id} is not a device id: {error}")))?;
    let (name, platform) = {
        let db = state.db();
        let seen = db.seen_devices().map_err(internal)?;
        match seen.iter().find(|row| row.device_id == device_id) {
            Some(row) => (row.display_name.clone(), row.platform.clone()),
            None => {
                let paired = db.paired_devices().map_err(internal)?;
                match paired.iter().find(|row| row.device_id == device_id) {
                    Some(row) => (row.display_name.clone(), row.platform.clone()),
                    None => {
                        return Err(internal(format!(
                            "{device_id} has not been seen or paired, so there is nothing to pair with"
                        )));
                    }
                }
            }
        }
    };
    // A card can outlive the row it came from. Starting an offer for a peer that is already in the records would show a
    // code for a decision that has already been made; the listener would refuse it, and the person would see a six-digit
    // number followed by "did not accept" for a device that is, in fact, still paired.
    let already = state
        .db()
        .paired_devices()
        .map_err(internal)?
        .iter()
        .any(|row| row.device_id == device_id);
    if already {
        return Err(internal(format!(
            "{name} is already paired; forget it on both devices before pairing again"
        )));
    }
    let Some(address) = state.host.lock().address_of(peer) else {
        return Err(internal(format!(
            "{name} is not reachable right now; run a scan and try again"
        )));
    };
    let platform = match platform.as_str() {
        "android" => synra_protocol::session::PlatformKind::Android,
        "ios" => synra_protocol::session::PlatformKind::Ios,
        _ => synra_protocol::session::PlatformKind::Desktop,
    };
    let desk = state.host.lock().pairing();
    let (self_id, self_name, self_platform) = {
        let host = state.host.lock();
        let local = host.local_peer();
        (local.device_id, local.display_name.clone(), local.platform)
    };
    let offer = desk
        .begin(peer, name.clone(), platform, already)
        .map_err(|refused| internal(format!("pairing could not start: {refused:?}")))?;
    // ★ 信封里「谁发的」必须是本机：桌子只负责码与对端，身份由这一层填。
    let request =
        synra_protocol::pairing::PairRequest::new(offer.code, self_id, self_name, self_platform);
    let snapshot = snapshot_of(&desk);

    // The dial goes on its own thread: this command's answer is the code, and the code already exists.
    let worker = Arc::clone(state);
    std::thread::spawn(move || {
        let data_dir = worker.paths.data_dir();
        synra_observe::wire::note(peer.to_string(), format!("dial {address} · {name}"));
        let outcome = synra_core::pairing_dial::dial(
            &worker.host,
            address,
            peer,
            &name,
            platform,
            &request,
            &data_dir,
        );
        if let Err(error) = &outcome {
            synra_observe::wire::note(peer.to_string(), format!("dial failed · {error:?}"));
            eprintln!("synra: the pairing dial failed: {error:?}");
        }
        worker.publish(synra_ipc::event::KernelEvent::PairingUpdated(Box::new(
            snapshot_of(&desk),
        )));
    });

    Ok(snapshot)
}

/// 接受或拒绝对面发来的请求。
///
/// # Errors
///
/// [`IpcError`] 当桌上没有等着答复的请求（已经答复过、已经过期）。
pub fn devices_pairing_respond(
    state: &Arc<AppState>,
    accepted: bool,
    block: bool,
) -> Result<PairingSnapshot, IpcError> {
    let desk = state.host.lock().pairing();
    if block {
        if accepted {
            return Err(internal("a blocked request cannot be accepted"));
        }
        let ask = desk
            .ask()
            .ok_or_else(|| internal("there is no pairing request waiting for an answer"))?;
        state
            .db()
            .block_device(&synra_store::Paired {
                device_id: ask.peer.to_string(),
                display_name: ask.peer_name,
                platform: platform_text(ask.peer_platform),
                features: "[]".to_owned(),
                address: None,
                paired_at_ms: synra_store::sqlite::now_ms(),
            })
            .map_err(internal)?;
    }
    let outcome = if accepted {
        desk.accept()
    } else {
        desk.refuse()
    };
    if outcome.is_err() {
        return Err(internal(
            "there is no pairing request waiting for an answer".to_owned(),
        ));
    }
    let snapshot = snapshot_of(&desk);
    state.publish(synra_ipc::event::KernelEvent::PairingUpdated(Box::new(
        snapshot.clone(),
    )));
    if block {
        let cards = super::devices::devices_snapshot(state)?.devices;
        for card in cards.into_iter().filter(|card| card.blocked) {
            state.publish(synra_ipc::event::KernelEvent::DeviceCardUpdated(Box::new(
                card,
            )));
        }
    }
    Ok(snapshot)
}

/// 撤回本机发出的请求。
///
/// # Errors
///
/// [`IpcError`] 当本机没有正在等待的请求。
pub fn devices_pairing_cancel(state: &Arc<AppState>) -> Result<PairingSnapshot, IpcError> {
    let desk = state.host.lock().pairing();
    if desk.cancel().is_err() {
        return Err(internal(
            "there is no pairing request of this device to withdraw".to_owned(),
        ));
    }
    let snapshot = snapshot_of(&desk);
    state.publish(synra_ipc::event::KernelEvent::PairingUpdated(Box::new(
        snapshot.clone(),
    )));
    Ok(snapshot)
}

/// 从内核那张桌子读出界面要的形状。
pub(crate) fn snapshot_of(desk: &Service) -> PairingSnapshot {
    let phase = match desk.phase() {
        synra_core::pairing::Phase::Idle => PairingPhase::Idle,
        synra_core::pairing::Phase::Waiting => PairingPhase::Waiting,
        synra_core::pairing::Phase::Asked => PairingPhase::Asked,
        synra_core::pairing::Phase::Done => PairingPhase::Done,
    };
    let offer = desk.offer().map(|offer| PairingOffer {
        code: offer.code.as_text(),
        device_id: offer.peer,
        display_name: offer.peer_name.clone(),
        platform: platform_text(offer.peer_platform),
    });
    let ask = desk.ask().map(|ask| PairingAsk {
        code: ask.code.as_text(),
        device_id: ask.peer,
        display_name: ask.peer_name.clone(),
        platform: platform_text(ask.peer_platform),
    });
    let outcome = match (desk.accepted(), desk.cancelled()) {
        (Some(true), _) => Some(PairingOutcome::Paired),
        (_, true) => Some(PairingOutcome::Cancelled),
        (Some(false), false) => Some(PairingOutcome::Refused),
        (None, false) => None,
    };
    PairingSnapshot {
        phase,
        offer,
        ask,
        outcome,
    }
}

/// 平台的显示文本。与存储层用同一组词，免得同一台设备在两个地方叫两个名字。
fn platform_text(platform: synra_protocol::session::PlatformKind) -> String {
    match platform {
        synra_protocol::session::PlatformKind::Android => "android",
        synra_protocol::session::PlatformKind::Ios => "ios",
        _ => "desktop",
    }
    .to_owned()
}

#[cfg(test)]
mod tests;
