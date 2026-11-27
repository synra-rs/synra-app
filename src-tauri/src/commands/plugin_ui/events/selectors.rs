use synra_ipc::{IpcError, event::PluginEvent};
use synra_plugin_ui::ValidatedSession;
use synra_protocol::{TransferId, event::EventName, inbound::HandlePattern};

use super::super::{require_grant, transfer::TRANSFER_GRANT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Topic {
    Devices,
    Transfers,
}

impl Topic {
    fn name(self) -> &'static str {
        match self {
            Self::Devices => PluginEvent::DEVICES_CHANGED,
            Self::Transfers => PluginEvent::TRANSFER_PROGRESS,
        }
    }

    fn grant(self) -> &'static str {
        match self {
            Self::Devices => "device:list",
            Self::Transfers => TRANSFER_GRANT,
        }
    }

    pub(super) fn allowed(self, session: &ValidatedSession) -> bool {
        require_grant(session, self.grant(), "event.poll").is_ok()
    }
}

#[derive(Clone)]
pub(super) enum Selector {
    Pattern(HandlePattern),
    Transfer(TransferId),
}

impl Selector {
    pub(super) fn allows(&self, session: &ValidatedSession) -> Result<(), IpcError> {
        if let Self::Pattern(pattern) = self
            && let Ok(name) = EventName::parse(pattern.as_str())
            && name.is_plugin()
        {
            return require_grant(session, &custom_grant(name.as_str()), "event.subscribe");
        }
        let mut matched = [Topic::Devices, Topic::Transfers]
            .into_iter()
            .filter(|topic| self.observes(*topic));
        let Some(first) = matched.next() else {
            return Ok(());
        };
        if first.allowed(session) || matched.any(|topic| topic.allowed(session)) {
            return Ok(());
        }
        require_grant(session, first.grant(), "event.subscribe")
    }

    pub(super) fn observes(&self, topic: Topic) -> bool {
        match self {
            Self::Pattern(pattern) => pattern.matches(topic.name()),
            Self::Transfer(_) => topic == Topic::Transfers,
        }
    }

    pub(super) fn transfer(&self, id: TransferId) -> bool {
        match self {
            Self::Pattern(pattern) => pattern.matches(Topic::Transfers.name()),
            Self::Transfer(selected) => *selected == id,
        }
    }

    pub(super) fn matches(&self, event: &PluginEvent) -> bool {
        match event {
            PluginEvent::DevicesChanged(_) => self.observes(Topic::Devices),
            PluginEvent::TransferProgress(status) => self.transfer(status.transfer_id),
            PluginEvent::Custom(_) => match self {
                Self::Pattern(pattern) => pattern.matches(event.name()),
                Self::Transfer(_) => false,
            },
            _ => false,
        }
    }
}

pub(super) fn custom_grant(name: &str) -> String {
    format!("event:subscribe:{name}")
}
