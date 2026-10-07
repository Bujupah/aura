//! Starts and stops listening, and relays the session's events to the tray
//! and the webviews. The webviews only ever see state, levels and text —
//! never audio and never the credential.

use std::sync::{Mutex, PoisonError};

use aura_session::{
    AudioInput, Credentials, FailureReason, Incoming, ListeningState, MeetingSession, Resume,
    SessionEnd, SessionEvent, SessionOptions, Translation,
};
use aura_storage::SavedSession;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::settings::{IncomingTranslation, Settings, SettingsStore};
use crate::tokens::{self, Provider};
use crate::{sessions, summary, topics, tray};

pub const EVENT: &str = "meeting://event";

/// Debug builds only: a directory holding `seller.pcm` and `customer.pcm` to
/// play instead of capturing this Mac's audio.
#[cfg(debug_assertions)]
const FIXTURES_ENV: &str = "AURA_AUDIO_FIXTURES";

pub struct Meeting {
    state: Mutex<ListeningState>,
    session: tokio::sync::Mutex<Option<MeetingSession>>,
    /// What the last virtual-microphone test found, for the menu to show.
    microphone_test: Mutex<Option<String>>,
}

impl Default for Meeting {
    fn default() -> Self {
        Self {
            state: Mutex::new(ListeningState::Idle),
            session: tokio::sync::Mutex::new(None),
            microphone_test: Mutex::new(None),
        }
    }
}

impl Meeting {
    pub fn state(&self) -> ListeningState {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn microphone_test(&self) -> Option<String> {
        self.microphone_test.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_microphone_test(&self, result: String) {
        *self.microphone_test.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
    }
}

#[tauri::command]
pub fn meeting_state(meeting: State<'_, Meeting>) -> ListeningState {
    meeting.state()
}

#[tauri::command]
pub async fn meeting_start(app: AppHandle) {
    start(&app, None).await;
}

/// Starts listening as a continuation of `saved`. Refused while a meeting
/// is already under way.
pub async fn resume(app: &AppHandle, saved: SavedSession) -> Result<(), String> {
    if !matches!(app.state::<Meeting>().state(), ListeningState::Idle | ListeningState::Failed { .. }) {
        return Err("Stop the current meeting before continuing another one.".into());
    }
    start(app, Some(saved)).await;
    Ok(())
}

#[tauri::command]
pub async fn meeting_stop(app: AppHandle) {
    stop(&app).await;
}

/// Passes the seller's removal of a topic on to the note-taking agent.
pub fn dismiss_topic(app: &AppHandle, topic_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(session) = app.state::<Meeting>().session.lock().await.as_ref() {
            session.dismiss_topic(topic_id);
        }
    });
}

/// For inputs that cannot await, such as the tray menu.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match app.state::<Meeting>().state() {
            ListeningState::Idle | ListeningState::Failed { .. } => start(&app, None).await,
            ListeningState::Starting | ListeningState::Listening => stop(&app).await,
        }
    });
}

async fn start(app: &AppHandle, saved: Option<SavedSession>) {
    let meeting = app.state::<Meeting>();
    let mut session = meeting.session.lock().await;
    if session.is_some() {
        return;
    }
    set_state(app, ListeningState::Starting);
    // A new session starts with a clean desk; a continued one gets back the
    // transcript and notes it ended with.
    match &saved {
        None => topics::reset(app),
        Some(saved) => {
            for turn in &saved.turns {
                emit(app, &SessionEvent::Turn { turn: turn.clone() });
            }
            topics::restore(app, saved.topics.clone());
            emit(app, &SessionEvent::Topics { topics: saved.topics.clone() });
        }
    }

    let settings = app.state::<SettingsStore>().get();
    let (credentials, options) = match (credentials(), options(&settings)) {
        (Ok(credentials), Ok(mut options)) => {
            options.resume = saved.as_ref().map(|saved| Resume {
                turns: saved.turns.clone(),
                topics: saved.topics.clone(),
            });
            (credentials, options)
        }
        (Err(message), _) => return fail(app, FailureReason::Credential, message),
        (_, Err(message)) => return fail(app, FailureReason::AudioOutput, message),
    };
    tracing::info!(
        event = "listening_starting",
        web_access = options.web_access,
        illustrations = options.illustrations,
        translating = options.translation.is_some(),
        continuing = options.resume.is_some()
    );
    sessions::begin(app, saved);
    let (events, received) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(relay(app.clone(), received));

    match MeetingSession::start(&credentials, audio_input(), options, events).await {
        Ok(started) => *session = Some(started),
        Err(error) => fail(app, error.reason(), error.to_string()),
    }
}

async fn stop(app: &AppHandle) {
    let ended = end_session(app).await;
    set_state(app, ListeningState::Idle);
    if let Some(ended) = ended {
        let session_id = sessions::finish(app);
        summary::prepare(app, ended.transcript, session_id);
    }
}

/// Sends one of the palette's requests to the advisor. Does nothing useful
/// when Aura is not listening, and says so.
#[tauri::command]
pub async fn meeting_ask(app: AppHandle, ask: aura_session::Ask, question: Option<String>) -> Result<(), String> {
    let question = question.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
    match app.state::<Meeting>().session.lock().await.as_ref() {
        Some(session) => {
            tracing::info!(event = "request_asked", command = ?ask);
            session.ask(ask, question);
            crate::shell::dispatch(
                &app,
                aura_core::shell::ShellCommand::SetOverlayMode {
                    mode: aura_core::shell::OverlayMode::Expanded,
                },
            );
            if let Err(error) = app.emit(EVENT, serde_json::json!({ "type": "answerAsked" })) {
                tracing::warn!(event = "meeting_emit_failed", %error);
            }
            Ok(())
        }
        None => Err("Start listening first: there is no meeting to work from yet.".into()),
    }
}

/// Asks the advisor what discovery has not established yet. Does nothing
/// when Aura is not listening.
#[tauri::command]
pub async fn meeting_whats_missing(app: AppHandle) -> Result<(), String> {
    match app.state::<Meeting>().session.lock().await.as_ref() {
        Some(session) => {
            session.ask_whats_missing();
            // The answer appears in the overlay, so make sure it is open and
            // shows that the question was heard, whichever window asked.
            crate::shell::dispatch(
                &app,
                aura_core::shell::ShellCommand::SetOverlayMode {
                    mode: aura_core::shell::OverlayMode::Expanded,
                },
            );
            if let Err(error) = app.emit(EVENT, serde_json::json!({ "type": "gapsAsked" })) {
                tracing::warn!(event = "meeting_emit_failed", %error);
            }
            Ok(())
        }
        None => Err("Start listening first: there is no meeting to review yet.".into()),
    }
}

/// Releases the microphone and closes the realtime sessions, leaving the
/// reported state as it is. Returns what the session left behind.
async fn end_session(app: &AppHandle) -> Option<SessionEnd> {
    let session = app.state::<Meeting>().session.lock().await.take()?;
    let ended = session.stop().await;
    tracing::info!(
        event = "listening_stopped",
        billed_seconds = ended.usage.seconds,
        usage_confirmed = ended.usage.confirmed,
        turns = ended.transcript.len()
    );
    Some(ended)
}

async fn relay(app: AppHandle, mut received: mpsc::UnboundedReceiver<SessionEvent>) {
    while let Some(event) = received.recv().await {
        match event {
            SessionEvent::State { state } => {
                let failed = matches!(state, ListeningState::Failed { .. });
                set_state(&app, state);
                if failed {
                    // A failed session must not keep the microphone open.
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = end_session(&app).await;
                        // What was said before the failure is still worth keeping.
                        sessions::finish(&app);
                    });
                }
            }
            SessionEvent::Topics { topics } => {
                sessions::record_topics(&app, &topics);
                topics::update(&app, topics.clone());
                emit(&app, &SessionEvent::Topics { topics });
            }
            // The picture stays in the core; windows fetch it when the
            // following topics event says it is ready.
            SessionEvent::TopicImage { topic_id, png } => topics::store_image(&app, topic_id, png),
            event => {
                if let SessionEvent::Turn { turn } = &event {
                    if turn.is_final {
                        sessions::record_turn(&app, turn);
                    }
                }
                emit(&app, &event);
            }
        }
    }
}

fn set_state(app: &AppHandle, state: ListeningState) {
    let meeting = app.state::<Meeting>();
    {
        let mut current = meeting.state.lock().unwrap_or_else(PoisonError::into_inner);
        // Once a failure is shown, only a deliberate start or stop replaces
        // it; a late "listening" from a session being torn down must not.
        let stale = matches!(*current, ListeningState::Failed { .. })
            && matches!(state, ListeningState::Listening);
        if stale || *current == state {
            return;
        }
        *current = state.clone();
    }
    let status = match &state {
        ListeningState::Idle => "idle",
        ListeningState::Starting => "starting",
        ListeningState::Listening => "listening",
        ListeningState::Failed { .. } => "failed",
    };
    tracing::info!(event = "listening_state", status);
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || tray::refresh(&handle)) {
        tracing::warn!(event = "tray_sync_failed", %error);
    }
    emit(app, &SessionEvent::State { state });
}

fn fail(app: &AppHandle, reason: FailureReason, message: String) {
    tracing::error!(event = "listening_failed", ?reason, %message);
    set_state(app, ListeningState::Failed { reason, message });
}

fn emit(app: &AppHandle, event: &SessionEvent) {
    if let Err(error) = app.emit(EVENT, event) {
        tracing::error!(event = "meeting_emit_failed", %error);
    }
}

pub(crate) fn credentials() -> Result<Credentials, String> {
    // Developers keep tokens in the repository's gitignored `.env`. Release
    // builds never read it. A token entered in the app wins over either.
    #[cfg(debug_assertions)]
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env"));

    Ok(Credentials {
        openai: tokens::resolve(Provider::Openai)
            .ok_or("Aura has no OpenAI token. Add one from the menu bar: API Tokens…")?,
        // Only translation needs it; its absence is reported if translation
        // is actually switched on.
        gemini: tokens::resolve(Provider::Gemini),
    })
}

/// The name fragment of the virtual audio device Aura speaks into so a
/// meeting app can use it as a microphone.
const VIRTUAL_MICROPHONE: &str = "BlackHole";

fn options(settings: &Settings) -> Result<SessionOptions, String> {
    let translation = if settings.translating() {
        // Sound sent to the virtual microphone is silent to the seller and
        // audible to the meeting. If the whole Mac plays into it, the seller
        // hears nothing and the participants hear themselves.
        if sound_output_is_virtual_microphone() {
            return Err(
                "Your Mac's sound output is set to BlackHole, so you would hear nothing and the meeting \
                 would hear itself. In System Settings → Sound → Output choose your headphones or \
                 speakers. BlackHole belongs only in your meeting app, as its microphone. Then start again."
                    .into(),
            );
        }
        let outgoing_device_uid = if settings.translate_my_voice {
            Some(virtual_microphone().ok_or(
                "To let the meeting hear your translated voice, Aura needs the BlackHole virtual \
                 audio device. Install BlackHole, choose \"BlackHole 2ch\" as the microphone in your \
                 meeting app, then start again — or turn off \"Translate my voice\" in the menu bar.",
            )?)
        } else {
            None
        };
        Some(Translation {
            my_language: settings.my_language.clone(),
            meeting_language: settings.meeting_language.clone(),
            incoming: match settings.incoming_translation {
                IncomingTranslation::Off => Incoming::Off,
                IncomingTranslation::Text => Incoming::Text,
                IncomingTranslation::Voice => Incoming::Voice,
            },
            outgoing_device_uid,
        })
    } else {
        None
    };
    Ok(SessionOptions {
        web_access: settings.web_access,
        illustrations: settings.illustrations,
        translation,
        resume: None,
    })
}

#[cfg(target_os = "macos")]
fn virtual_microphone() -> Option<String> {
    aura_audio::playback::find_output_device(VIRTUAL_MICROPHONE)
}

#[cfg(not(target_os = "macos"))]
fn virtual_microphone() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn sound_output_is_virtual_microphone() -> bool {
    match (aura_audio::playback::default_output_device(), virtual_microphone()) {
        (Some(output), Some(microphone)) => output == microphone,
        _ => false,
    }
}

#[cfg(not(target_os = "macos"))]
fn sound_output_is_virtual_microphone() -> bool {
    false
}

/// Plays a tone into the virtual microphone and listens for it there, then
/// says in plain words what was found. Takes about two seconds.
pub async fn test_virtual_microphone() -> String {
    #[cfg(target_os = "macos")]
    {
        use aura_audio::playback::{loopback_test, Loopback};
        let Some(device) = virtual_microphone() else {
            return "Not installed. Install BlackHole 2ch, then restart your Mac.".into();
        };
        let output_misrouted = sound_output_is_virtual_microphone();
        let result = tauri::async_runtime::spawn_blocking(move || loopback_test(&device)).await;
        let verdict = match result {
            Ok(Loopback::Heard(_)) => "Working: sound Aura sends reaches the virtual microphone.",
            Ok(Loopback::Silent) => "Not working: Aura played a tone into BlackHole and nothing came out of it.",
            Ok(Loopback::NoPermission) => {
                "Can't test: allow Aura to use the microphone in System Settings → Privacy & Security → Microphone."
            }
            Ok(Loopback::NoDevice) => "Not working: BlackHole has no microphone side on this Mac. Try reinstalling it.",
            Ok(Loopback::Failed) | Err(_) => "The test could not run.",
        };
        tracing::info!(event = "virtual_microphone_tested", result = ?result.ok(), output_misrouted);
        if output_misrouted {
            format!("{verdict} But your Mac's sound output is BlackHole: switch it to your headphones.")
        } else {
            verdict.to_owned()
        }
    }
    #[cfg(not(target_os = "macos"))]
    "Virtual microphones are only supported on macOS.".into()
}

fn audio_input() -> AudioInput {
    #[cfg(debug_assertions)]
    if let Some(directory) = std::env::var_os(FIXTURES_ENV).map(std::path::PathBuf::from) {
        return AudioInput::Fixtures {
            seller: directory.join("seller.pcm"),
            customer: directory.join("customer.pcm"),
        };
    }
    AudioInput::Capture
}
