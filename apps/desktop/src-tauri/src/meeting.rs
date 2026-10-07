//! Starts and stops listening, and relays the session's events to the tray
//! and the webviews. The webviews only ever see state, levels and text —
//! never audio and never the credential.

use std::sync::{Mutex, PoisonError};

use aura_live::ApiCredential;
use aura_session::{
    AudioInput, Credentials, FailureReason, Incoming, ListeningState, MeetingSession, SessionEvent,
    SessionOptions, Translation,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::settings::{IncomingTranslation, Settings, SettingsStore};
use crate::{topics, tray};

pub const EVENT: &str = "meeting://event";

/// Debug builds only: a directory holding `seller.pcm` and `customer.pcm` to
/// play instead of capturing this Mac's audio.
#[cfg(debug_assertions)]
const FIXTURES_ENV: &str = "AURA_AUDIO_FIXTURES";

pub struct Meeting {
    state: Mutex<ListeningState>,
    session: tokio::sync::Mutex<Option<MeetingSession>>,
}

impl Default for Meeting {
    fn default() -> Self {
        Self {
            state: Mutex::new(ListeningState::Idle),
            session: tokio::sync::Mutex::new(None),
        }
    }
}

impl Meeting {
    pub fn state(&self) -> ListeningState {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

#[tauri::command]
pub fn meeting_state(meeting: State<'_, Meeting>) -> ListeningState {
    meeting.state()
}

#[tauri::command]
pub async fn meeting_start(app: AppHandle) {
    start(&app).await;
}

#[tauri::command]
pub async fn meeting_stop(app: AppHandle) {
    stop(&app).await;
}

/// For inputs that cannot await, such as the tray menu.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match app.state::<Meeting>().state() {
            ListeningState::Idle | ListeningState::Failed { .. } => start(&app).await,
            ListeningState::Starting | ListeningState::Listening => stop(&app).await,
        }
    });
}

async fn start(app: &AppHandle) {
    let meeting = app.state::<Meeting>();
    let mut session = meeting.session.lock().await;
    if session.is_some() {
        return;
    }
    set_state(app, ListeningState::Starting);
    // A new session starts with a clean desk.
    topics::reset(app);

    let settings = app.state::<SettingsStore>().get();
    let (credentials, options) = match (credentials(), options(&settings)) {
        (Ok(credentials), Ok(options)) => (credentials, options),
        (Err(message), _) => return fail(app, FailureReason::Credential, message),
        (_, Err(message)) => return fail(app, FailureReason::AudioOutput, message),
    };
    tracing::info!(
        event = "listening_starting",
        web_access = options.web_access,
        illustrations = options.illustrations,
        translating = options.translation.is_some()
    );
    let (events, received) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(relay(app.clone(), received));

    match MeetingSession::start(&credentials, audio_input(), options, events).await {
        Ok(started) => *session = Some(started),
        Err(error) => fail(app, error.reason(), error.to_string()),
    }
}

async fn stop(app: &AppHandle) {
    end_session(app).await;
    set_state(app, ListeningState::Idle);
}

/// Releases the microphone and closes the realtime sessions, leaving the
/// reported state as it is.
async fn end_session(app: &AppHandle) {
    let session = app.state::<Meeting>().session.lock().await.take();
    if let Some(session) = session {
        let usage = session.stop().await;
        tracing::info!(
            event = "listening_stopped",
            billed_seconds = usage.seconds,
            usage_confirmed = usage.confirmed
        );
    }
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
                    tauri::async_runtime::spawn(async move { end_session(&app).await });
                }
            }
            SessionEvent::Topics { topics } => {
                topics::update(&app, topics.clone());
                emit(&app, &SessionEvent::Topics { topics });
            }
            // The picture stays in the core; windows fetch it when the
            // following topics event says it is ready.
            SessionEvent::TopicImage { topic_id, png } => topics::store_image(&app, topic_id, png),
            event => emit(&app, &event),
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

/// The Keychain service under which Aura's tokens are stored, one item per
/// token, with the token's name as the account.
const KEYCHAIN_SERVICE: &str = "dev.aura.desktop";

fn credentials() -> Result<Credentials, String> {
    // Developers keep the tokens in the repository's gitignored `.env`.
    // Release builds never read it.
    #[cfg(debug_assertions)]
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env"));

    let openai = token(aura_live::API_TOKEN_ENV).ok_or_else(|| {
        format!(
            "Aura has no OpenAI token. Add one to your Keychain by running this in Terminal, then \
             start again:  security add-generic-password -U -s {KEYCHAIN_SERVICE} -a {} -w",
            aura_live::API_TOKEN_ENV
        )
    })?;
    Ok(Credentials {
        openai,
        // Only translation needs it; its absence is reported if translation
        // is actually switched on.
        gemini: token(aura_translate::API_TOKEN_ENV),
    })
}

/// A token from the environment or, failing that, the macOS Keychain. Until
/// the gateway issues short-lived credentials, the Keychain is where a token
/// lives on a machine that has no development checkout.
fn token(name: &'static str) -> Option<ApiCredential> {
    if let Ok(credential) = ApiCredential::from_env_var(name) {
        return Some(credential);
    }
    #[cfg(target_os = "macos")]
    {
        let secret = security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, name).ok()?;
        ApiCredential::new(String::from_utf8(secret).ok()?).ok()
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// The name fragment of the virtual audio device Aura speaks into so a
/// meeting app can use it as a microphone.
const VIRTUAL_MICROPHONE: &str = "BlackHole";

fn options(settings: &Settings) -> Result<SessionOptions, String> {
    let translation = if settings.translating() {
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
