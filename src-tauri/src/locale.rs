//! Language selection shared by native presentation and plugin environments.

#[cfg(any(feature = "tauri-shell", test))]
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Language {
    English,
    Chinese,
}

impl Language {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
        }
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(crate) const fn show(self) -> &'static str {
        match self {
            Self::English => "Show Synra",
            Self::Chinese => "显示 Synra",
        }
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(crate) const fn quit(self) -> &'static str {
        match self {
            Self::English => "Quit",
            Self::Chinese => "退出",
        }
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(crate) fn received(self, name: &str) -> String {
        match self {
            Self::English => format!("Received {name}"),
            Self::Chinese => format!("已收到 {name}"),
        }
    }
}

fn resolve<I, S>(preference: Option<&str>, languages: impl FnOnce() -> I) -> Language
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    match preference {
        Some("en") => return Language::English,
        Some("zh-CN") => return Language::Chinese,
        _ => {}
    }
    for language in languages() {
        match language.as_ref().split(['-', '_']).next() {
            Some(base) if base.eq_ignore_ascii_case("zh") => return Language::Chinese,
            Some(base) if base.eq_ignore_ascii_case("en") => return Language::English,
            _ => {}
        }
    }
    Language::English
}

pub(crate) fn from_preference(preference: Option<&str>) -> Language {
    resolve(preference, sys_locale::get_locales)
}

#[cfg(any(feature = "tauri-shell", test))]
pub(crate) fn current(state: &AppState) -> Language {
    let preference = state
        .db()
        .config()
        .ok()
        .and_then(|rows| rows.into_iter().find(|row| row.key == "ui.locale"))
        .and_then(|row| serde_json::from_str::<String>(&row.value).ok());
    from_preference(preference.as_deref())
}

#[cfg(test)]
mod tests {
    use super::{Language, current, resolve};

    #[test]
    fn explicit_preferences_do_not_query_the_operating_system() {
        assert_eq!(
            resolve(Some("en"), || -> [&str; 0] {
                panic!("unneeded locale query")
            }),
            Language::English
        );
        assert_eq!(
            resolve(Some("zh-CN"), || -> [&str; 0] {
                panic!("unneeded locale query")
            }),
            Language::Chinese
        );
    }

    #[test]
    fn system_selection_respects_supported_language_priority() {
        for preference in [None, Some("system"), Some("invalid")] {
            assert_eq!(
                resolve(preference, || ["fr-FR", "zh_Hant_TW", "en-US"]),
                Language::Chinese
            );
            assert_eq!(
                resolve(preference, || ["en-GB", "zh-CN"]),
                Language::English
            );
            assert_eq!(resolve(preference, || ["fr-FR"]), Language::English);
            assert_eq!(resolve(preference, || [] as [&str; 0]), Language::English);
        }
    }

    #[test]
    fn native_surfaces_and_plugin_codes_use_the_same_language() {
        assert_eq!(Language::Chinese.code(), "zh-CN");
        assert_eq!(Language::Chinese.show(), "显示 Synra");
        assert_eq!(Language::Chinese.quit(), "退出");
        assert_eq!(Language::Chinese.received("notes.txt"), "已收到 notes.txt");
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::English.received("笔记.txt"), "Received 笔记.txt");
    }

    #[test]
    fn current_language_is_read_again_after_settings_change() {
        let state = crate::testing::state();
        state
            .db()
            .set_config("ui.locale", &serde_json::json!("zh-CN"), 1)
            .unwrap();
        assert_eq!(current(&state), Language::Chinese);
        state
            .db()
            .set_config("ui.locale", &serde_json::json!("en"), 2)
            .unwrap();
        assert_eq!(current(&state), Language::English);
    }
}
