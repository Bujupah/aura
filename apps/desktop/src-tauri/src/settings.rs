//! The seller's choices, kept in a small JSON file so they survive a
//! restart. No secrets live here.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::languages;

pub const EVENT: &str = "settings://changed";
const FILE: &str = "settings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncomingTranslation {
    Off,
    /// Show what the meeting says in my language.
    Text,
    /// Also speak it to me.
    Voice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The language I speak and read.
    pub my_language: String,
    /// The language the meeting is held in.
    pub meeting_language: String,
    pub incoming_translation: IncomingTranslation,
    /// Translate my speech into the meeting's language for the others.
    pub translate_my_voice: bool,
    pub web_access: bool,
    pub illustrations: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            my_language: "en".into(),
            meeting_language: "en".into(),
            incoming_translation: IncomingTranslation::Off,
            translate_my_voice: false,
            web_access: true,
            illustrations: true,
        }
    }
}

impl Settings {
    /// Translation only means something between two different languages.
    pub fn translating(&self) -> bool {
        self.my_language != self.meeting_language
            && (self.incoming_translation != IncomingTranslation::Off || self.translate_my_voice)
    }

    /// Replaces anything a hand-edited or outdated file got wrong.
    fn repaired(mut self) -> Self {
        let fallback = Self::default();
        if !languages::is_supported(&self.my_language) {
            self.my_language = fallback.my_language;
        }
        if !languages::is_supported(&self.meeting_language) {
            self.meeting_language = fallback.meeting_language;
        }
        self
    }
}

#[derive(Default)]
pub struct SettingsStore {
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn get(&self) -> Settings {
        self.current.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|directory| directory.join(FILE))
}

/// Reads the saved settings, falling back to defaults for anything missing
/// or unreadable.
pub fn load(app: &AppHandle) {
    let loaded = path(app)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default()
        .repaired();
    *app.state::<SettingsStore>().current.lock().unwrap_or_else(PoisonError::into_inner) = loaded;
}

/// Applies a change, saves it, and tells the webviews.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut Settings)) -> Settings {
    let settings = {
        let store = app.state::<SettingsStore>();
        let mut current = store.current.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut current);
        *current = current.clone().repaired();
        current.clone()
    };
    if let Some(path) = path(app) {
        let saved = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap_or_default()));
        if let Err(error) = saved {
            tracing::warn!(event = "settings_save_failed", %error);
        }
    }
    if let Err(error) = app.emit(EVENT, &settings) {
        tracing::warn!(event = "settings_emit_failed", %error);
    }
    settings
}

#[tauri::command]
pub fn settings_get(store: tauri::State<'_, SettingsStore>) -> Settings {
    store.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_needs_two_languages_and_something_switched_on() {
        let mut settings = Settings::default();
        assert!(!settings.translating());
        settings.incoming_translation = IncomingTranslation::Text;
        assert!(!settings.translating(), "same language on both sides");
        settings.meeting_language = "es".into();
        assert!(settings.translating());
        settings.incoming_translation = IncomingTranslation::Off;
        assert!(!settings.translating());
        settings.translate_my_voice = true;
        assert!(settings.translating());
    }

    #[test]
    fn a_partial_or_damaged_file_still_loads() {
        let settings: Settings = serde_json::from_str(r#"{"meetingLanguage":"es"}"#).unwrap();
        assert_eq!(settings.meeting_language, "es");
        assert_eq!(settings.my_language, "en");
        assert!(settings.web_access);

        let repaired = serde_json::from_str::<Settings>(r#"{"myLanguage":"klingon"}"#).unwrap().repaired();
        assert_eq!(repaired.my_language, "en");
    }

    #[test]
    fn every_listed_language_has_a_distinct_code() {
        let mut codes: Vec<&str> = languages::LANGUAGES.iter().map(|(code, _)| *code).collect();
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total);
        assert!(languages::is_supported("en") && languages::is_supported("es"));
    }
}
