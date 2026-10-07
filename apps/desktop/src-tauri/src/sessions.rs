//! Saved meetings: recording the one in progress, and browsing, deleting and
//! continuing earlier ones.
//!
//! A session is its transcript, topic notes and summary — never audio. It is
//! written encrypted, with a key that lives in the Keychain, and only while
//! the seller has saving switched on.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aura_core::summary::MeetingSummary;
use aura_core::topics::Topic;
use aura_core::transcript::Turn;
use aura_storage::{SavedSession, SessionMeta, SessionStore, KEY_BYTES};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::settings::SettingsStore;
use crate::{meeting, tokens};

pub const EVENT: &str = "sessions://changed";
pub const WINDOW: &str = "sessions";
const KEY_ACCOUNT: &str = "SESSION_STORE_KEY";
const PROVISIONAL_TITLE: &str = "Untitled meeting";
/// While listening, the session on disk is never older than this.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct Sessions {
    store: Mutex<Option<Arc<SessionStore>>>,
    /// The meeting being recorded, if saving is on.
    active: Mutex<Option<Active>>,
}

struct Active {
    session: SavedSession,
    saved_at: Instant,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// The store, opened on first use. Its key is created once and kept in the
/// Keychain.
fn store(app: &AppHandle) -> Result<Arc<SessionStore>, String> {
    let sessions = app.state::<Sessions>();
    let mut slot = lock(&sessions.store);
    if let Some(store) = slot.as_ref() {
        return Ok(store.clone());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Aura has nowhere to keep sessions: {error}"))?
        .join("sessions");
    let store = Arc::new(SessionStore::open(directory, &key()?).map_err(|error| error.to_string())?);
    *slot = Some(store.clone());
    Ok(store)
}

fn key() -> Result<[u8; KEY_BYTES], String> {
    if let Some(saved) = tokens::from_keychain(KEY_ACCOUNT) {
        return from_hex(&saved).ok_or_else(|| "The key for saved sessions in the Keychain is damaged.".to_owned());
    }
    let key = aura_storage::generate_key();
    tokens::to_keychain(KEY_ACCOUNT, &to_hex(&key))?;
    Ok(key)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Option<[u8; KEY_BYTES]> {
    let text = text.trim();
    if text.len() != KEY_BYTES * 2 || !text.is_ascii() {
        return None;
    }
    let mut key = [0u8; KEY_BYTES];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(key)
}

/// Starts recording a meeting: a new one, or `resume` continued.
pub fn begin(app: &AppHandle, resume: Option<SavedSession>) {
    if !app.state::<SettingsStore>().get().save_sessions {
        *lock(&app.state::<Sessions>().active) = None;
        return;
    }
    let now = now_ms();
    let session = resume.unwrap_or_else(|| SavedSession {
        id: format!("s-{now}"),
        title: PROVISIONAL_TITLE.to_owned(),
        started_at_ms: now,
        updated_at_ms: now,
        turns: Vec::new(),
        topics: Vec::new(),
        summary: None,
    });
    *lock(&app.state::<Sessions>().active) = Some(Active {
        session,
        saved_at: Instant::now(),
    });
}

/// Adds a finalized turn, or replaces it if it was already recorded (an
/// interpretation can arrive after its turn closed).
pub fn record_turn(app: &AppHandle, turn: &Turn) {
    record(app, |session| match session.turns.iter_mut().find(|existing| existing.id == turn.id) {
        Some(existing) => *existing = turn.clone(),
        None => session.turns.push(turn.clone()),
    });
}

pub fn record_topics(app: &AppHandle, topics: &[Topic]) {
    record(app, |session| session.topics = storable(topics));
}

/// Pictures are not kept, so a saved topic does not claim to have one.
fn storable(topics: &[Topic]) -> Vec<Topic> {
    topics
        .iter()
        .map(|topic| Topic {
            image: None,
            ..topic.clone()
        })
        .collect()
}

fn record(app: &AppHandle, change: impl FnOnce(&mut SavedSession)) {
    let due = {
        let sessions = app.state::<Sessions>();
        let mut active = lock(&sessions.active);
        let Some(active) = active.as_mut() else {
            return;
        };
        change(&mut active.session);
        active.session.updated_at_ms = now_ms();
        if active.saved_at.elapsed() < AUTOSAVE_EVERY {
            return;
        }
        active.saved_at = Instant::now();
        active.session.clone()
    };
    write(app, &due);
}

/// Ends recording and saves the meeting. Returns its id, or `None` if
/// nothing was said or saving is off.
pub fn finish(app: &AppHandle) -> Option<String> {
    let mut session = lock(&app.state::<Sessions>().active).take()?.session;
    if session.turns.is_empty() {
        return None;
    }
    if session.title == PROVISIONAL_TITLE {
        session.title = working_title(&session.turns);
    }
    session.updated_at_ms = now_ms();
    write(app, &session);
    Some(session.id)
}

/// A name to go by until the summary supplies a better one: how the meeting
/// opened.
fn working_title(turns: &[Turn]) -> String {
    const MAX_CHARS: usize = 48;
    let opening = turns.first().map(|turn| turn.text.trim()).unwrap_or_default();
    if opening.is_empty() {
        return PROVISIONAL_TITLE.to_owned();
    }
    if opening.chars().count() <= MAX_CHARS {
        return opening.to_owned();
    }
    let clipped: String = opening.chars().take(MAX_CHARS - 1).collect();
    format!("{}…", clipped.trim_end())
}

/// Attaches the finished summary, whose headline becomes the session's name.
pub fn apply_summary(app: &AppHandle, session_id: &str, summary: &MeetingSummary) {
    // If that meeting is already being continued, the copy in memory is the
    // one that will be saved next; it must carry the name and summary too.
    if let Some(active) = lock(&app.state::<Sessions>().active).as_mut() {
        if active.session.id == session_id {
            if !summary.headline.trim().is_empty() {
                active.session.title = summary.headline.clone();
            }
            active.session.summary = Some(summary.clone());
        }
    }
    let updated = store(app).and_then(|store| {
        let mut session = store.load(session_id).map_err(|error| error.to_string())?;
        if !summary.headline.trim().is_empty() {
            session.title = summary.headline.clone();
        }
        session.summary = Some(summary.clone());
        session.updated_at_ms = now_ms();
        store.save(&session).map_err(|error| error.to_string())
    });
    match updated {
        Ok(()) => changed(app),
        Err(error) => tracing::warn!(event = "session_summary_not_saved", %error),
    }
}

fn write(app: &AppHandle, session: &SavedSession) {
    match store(app).and_then(|store| store.save(session).map_err(|error| error.to_string())) {
        Ok(()) => {
            tracing::info!(event = "session_saved", turns = session.turns.len(), topics = session.topics.len());
            changed(app);
        }
        Err(error) => tracing::error!(event = "session_not_saved", %error),
    }
}

fn changed(app: &AppHandle) {
    if let Err(error) = app.emit(EVENT, ()) {
        tracing::warn!(event = "sessions_emit_failed", %error);
    }
}

#[tauri::command]
pub fn sessions_list(app: AppHandle) -> Result<Vec<SessionMeta>, String> {
    store(&app)?.list().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn session_get(app: AppHandle, id: String) -> Result<SavedSession, String> {
    store(&app)?.load(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn session_delete(app: AppHandle, id: String) -> Result<(), String> {
    store(&app)?.delete(&id).map_err(|error| error.to_string())?;
    tracing::info!(event = "session_deleted");
    changed(&app);
    Ok(())
}

/// Starts listening again as a continuation of a saved meeting.
#[tauri::command]
pub async fn session_continue(app: AppHandle, id: String) -> Result<(), String> {
    let session = store(&app)?.load(&id).map_err(|error| error.to_string())?;
    meeting::resume(&app, session).await
}

#[tauri::command]
pub fn sessions_open(app: AppHandle) {
    open_window(&app);
}

/// An ordinary window: the seller reads, selects and scrolls in it.
/// Must run on the main thread.
pub fn open_window(app: &AppHandle) {
    let shown = match app.get_webview_window(WINDOW) {
        Some(window) => window.show().and_then(|()| window.set_focus()),
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html".into()))
            .title("Aura — Sessions")
            .inner_size(860.0, 620.0)
            .min_inner_size(640.0, 420.0)
            .center()
            .focused(true)
            // Best effort only, like every Aura window.
            .content_protected(true)
            .build()
            .and_then(|window| window.set_focus()),
    };
    if let Err(error) = shown {
        tracing::error!(event = "sessions_window_failed", %error);
    }
}

/// Emergency hide covers this window too. Must run on the main thread.
pub fn close_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::transcript::Speaker;

    fn turn(text: &str) -> Turn {
        Turn {
            id: "customer-0".into(),
            speaker: Speaker::Customer,
            text: text.into(),
            start_ms: 0,
            end_ms: 1,
            is_final: true,
            translation: None,
        }
    }

    #[test]
    fn the_store_key_round_trips_through_its_keychain_form() {
        let key = aura_storage::generate_key();
        assert_eq!(from_hex(&to_hex(&key)), Some(key));
        assert_eq!(from_hex("too short"), None);
        assert_eq!(from_hex(&"zz".repeat(KEY_BYTES)), None);
    }

    #[test]
    fn a_meeting_is_named_after_how_it_opened_until_it_has_a_summary() {
        assert_eq!(working_title(&[turn("  Our CMDB is stale. ")]), "Our CMDB is stale.");
        let long = working_title(&[turn(&"word ".repeat(40))]);
        assert!(long.chars().count() <= 48 && long.ends_with('…'));
        assert_eq!(working_title(&[]), PROVISIONAL_TITLE);
    }

    #[test]
    fn saved_topics_do_not_claim_a_picture_that_was_not_kept() {
        use aura_core::topics::{ImageStatus, TopicImage};
        let topic = Topic {
            id: "env".into(),
            title: "Environment".into(),
            notes: vec!["Runs AWS".into()],
            source_turn_ids: vec!["customer-0".into()],
            sources: Vec::new(),
            diagram: Some("flowchart TD\n  A --> B".into()),
            image: Some(TopicImage { brief: "x".into(), status: ImageStatus::Ready }),
            updated_at_ms: 1,
            placement: None,
        };
        let saved = storable(&[topic]);
        assert_eq!(saved[0].image, None);
        assert!(saved[0].diagram.is_some(), "diagrams are text and are kept");
    }
}
