//! Menu bar item. It is the one piece of Aura that stays visible during an
//! emergency hide, so it always reflects the real state — and it is where
//! the seller chooses languages and what the agent may do.
//!
//! The menu is rebuilt whenever something it shows changes, which keeps
//! every label and tick derived from one place.

use aura_core::shell::{OverlayMode, ShellCommand};
use aura_session::ListeningState;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::languages::{self, LANGUAGES};
use crate::meeting::{self, Meeting};
use crate::settings::{self, IncomingTranslation, SettingsStore};
use crate::shell::{self, Shell};

const TRAY_ID: &str = "aura";
const MY_LANGUAGE: &str = "language:mine:";
const MEETING_LANGUAGE: &str = "language:meeting:";

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("Aura")
        .menu(&build(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Rebuilds the menu from current state. Must run on the main thread.
pub fn refresh(app: &AppHandle) {
    let rebuilt = build(app).and_then(|menu| match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_menu(Some(menu)),
        None => Ok(()),
    });
    if let Err(error) = rebuilt {
        tracing::warn!(event = "tray_refresh_failed", %error);
    }
}

fn on_menu_event(app: &AppHandle, id: &str) {
    let command = match id {
        "listening" => return meeting::toggle(app),
        "hidden" => ShellCommand::ToggleHidden,
        "overlay" => ShellCommand::ToggleOverlayMode,
        "click_through" => ShellCommand::ToggleClickThrough,
        "topics" => ShellCommand::ToggleTopics,
        "palette" => ShellCommand::TogglePalette,
        "test_microphone" => {
            let app = app.clone();
            app.state::<Meeting>().set_microphone_test("Testing…".into());
            refresh(&app);
            tauri::async_runtime::spawn(async move {
                let result = meeting::test_virtual_microphone().await;
                app.state::<Meeting>().set_microphone_test(result);
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || refresh(&handle));
            });
            return;
        }
        "tokens" => return crate::tokens::open_window(app),
        "summary" => return crate::summary::open_window(app),
        "sessions" => return crate::sessions::open_window(app),
        "quit" => return app.exit(0),
        _ => {
            change_setting(app, id);
            // The event arrives on the main thread.
            return refresh(app);
        }
    };
    shell::dispatch(app, command);
}

fn change_setting(app: &AppHandle, id: &str) {
    let updated = settings::update(app, |settings| {
        if let Some(code) = id.strip_prefix(MY_LANGUAGE) {
            settings.my_language = code.to_owned();
        } else if let Some(code) = id.strip_prefix(MEETING_LANGUAGE) {
            settings.meeting_language = code.to_owned();
        } else {
            match id {
                "incoming:off" => settings.incoming_translation = IncomingTranslation::Off,
                "incoming:text" => settings.incoming_translation = IncomingTranslation::Text,
                "incoming:voice" => settings.incoming_translation = IncomingTranslation::Voice,
                "outgoing" => settings.translate_my_voice = !settings.translate_my_voice,
                "web_access" => settings.web_access = !settings.web_access,
                "illustrations" => settings.illustrations = !settings.illustrations,
                "save_sessions" => settings.save_sessions = !settings.save_sessions,
                _ => {}
            }
        }
    });
    tracing::info!(
        event = "settings_changed",
        my_language = %updated.my_language,
        meeting_language = %updated.meeting_language,
        translating = updated.translating()
    );
}

fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let shell = app.state::<Shell>().state();
    let listening = app.state::<Meeting>().state();
    let settings = app.state::<SettingsStore>().get();
    let item = |id: &str, text: &str, enabled: bool| MenuItem::with_id(app, id, text, enabled, None::<&str>);
    let check = |id: &str, text: &str, enabled: bool, checked: bool| {
        CheckMenuItem::with_id(app, id, text, enabled, checked, None::<&str>)
    };
    let separator = || PredefinedMenuItem::separator(app);

    let language_menu = |title: String, prefix: &str, selected: &str| -> tauri::Result<Submenu<Wry>> {
        let menu = Submenu::with_id(app, format!("menu:{prefix}"), title, true)?;
        for (code, name) in LANGUAGES {
            menu.append(&check(&format!("{prefix}{code}"), name, true, *code == selected)?)?;
        }
        Ok(menu)
    };

    // While hidden, only restoring is honoured; the menu should not offer more.
    let usable = !shell.hidden;
    let same_language = settings.my_language == settings.meeting_language;
    let translation = Submenu::with_id(
        app,
        "menu:translation",
        if settings.translating() {
            format!(
                "Translation: {} ⇄ {}",
                languages::name_of(&settings.my_language),
                languages::name_of(&settings.meeting_language)
            )
        } else {
            "Translation: Off".to_owned()
        },
        true,
    )?;
    translation.append_items(&[
        &language_menu(
            format!("I Speak: {}", languages::name_of(&settings.my_language)),
            MY_LANGUAGE,
            &settings.my_language,
        )?,
        &language_menu(
            format!("Meeting Language: {}", languages::name_of(&settings.meeting_language)),
            MEETING_LANGUAGE,
            &settings.meeting_language,
        )?,
        &separator()?,
        &item("label:incoming", "What the meeting says:", false)?,
        &check("incoming:off", "Leave as spoken", true, settings.incoming_translation == IncomingTranslation::Off)?,
        &check(
            "incoming:text",
            "Show it in my language (text only)",
            true,
            settings.incoming_translation == IncomingTranslation::Text,
        )?,
        &check(
            "incoming:voice",
            "Show it and speak it to me",
            true,
            settings.incoming_translation == IncomingTranslation::Voice,
        )?,
        &separator()?,
        &check("outgoing", "Translate my voice for the meeting", true, settings.translate_my_voice)?,
        &item("test_microphone", "Test Virtual Microphone", true)?,
        &item(
            "label:microphone_test",
            // Menus are single-line; the outcome is worded to fit.
            app.state::<Meeting>()
                .microphone_test()
                .as_deref()
                .unwrap_or("Plays a short tone into BlackHole and checks it arrives"),
            false,
        )?,
        &separator()?,
        &item("label:setup_1", "Setup: meeting app microphone = BlackHole 2ch", false)?,
        &item("label:setup_2", "Setup: Mac sound output = your headphones, never BlackHole", false)?,
        &separator()?,
        &item(
            "label:translation_note",
            if same_language {
                "Choose two different languages to translate"
            } else {
                "Changes apply from the next session"
            },
            false,
        )?,
    ])?;

    let agent = Submenu::with_id(app, "menu:agent", "Agent", true)?;
    agent.append_items(&[
        &check("web_access", "May search the web", true, settings.web_access)?,
        &check("illustrations", "May generate images", true, settings.illustrations)?,
        &item("label:agent_note", "Changes apply from the next session", false)?,
    ])?;

    Menu::with_items(
        app,
        &[
            &item(
                "listening",
                match listening {
                    ListeningState::Idle | ListeningState::Failed { .. } => "Start Listening",
                    ListeningState::Starting => "Starting… (Cancel)",
                    ListeningState::Listening => "● Listening — Stop",
                },
                true,
            )?,
            &translation,
            &agent,
            &item("sessions", "Sessions…", usable)?,
            &check("save_sessions", "Save Sessions", true, settings.save_sessions)?,
            &item("summary", "Last Meeting Summary…", usable && crate::summary::exists(app))?,
            &item("tokens", "API Tokens…", usable)?,
            &separator()?,
            &item(
                "hidden",
                if shell.hidden { "Restore Aura  ⌘⇧." } else { "Hide All Aura Windows  ⌘⇧." },
                true,
            )?,
            &item(
                "overlay",
                match shell.overlay_mode {
                    OverlayMode::Collapsed => "Expand Overlay",
                    OverlayMode::Expanded => "Collapse Overlay",
                },
                usable,
            )?,
            &item("palette", "Command Palette…", usable)?,
            &check("topics", "Show Topic Windows", usable, shell.topics_visible)?,
            &check("click_through", "Click-Through Overlay", usable, shell.click_through)?,
            &separator()?,
            &item("quit", "Quit Aura", true)?,
        ],
    )
}
