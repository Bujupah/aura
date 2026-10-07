//! The write-up shown when a meeting ends.
//!
//! It is prepared once listening stops, kept in memory until the next
//! meeting replaces it, and never written to disk; the seller copies out
//! what they want to keep.

use std::sync::{Mutex, PoisonError};

use aura_core::summary::MeetingSummary;
use aura_core::transcript::Turn;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::settings::SettingsStore;
use crate::{meeting, topics};

pub const EVENT: &str = "summary://state";
pub const WINDOW: &str = "summary";
/// A shorter exchange than this is not a meeting worth writing up.
const MIN_TURNS: usize = 3;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SummaryState {
    /// No meeting has ended yet.
    #[default]
    None,
    Preparing,
    Ready {
        summary: Box<MeetingSummary>,
        /// The same summary as Markdown, for copying.
        markdown: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Default)]
pub struct SummaryStore(Mutex<SummaryState>);

impl SummaryStore {
    pub fn get(&self) -> SummaryState {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

#[tauri::command]
pub fn summary_get(store: State<'_, SummaryStore>) -> SummaryState {
    store.get()
}

#[tauri::command]
pub fn summary_open(app: AppHandle) {
    open_window(&app);
}

pub fn exists(app: &AppHandle) -> bool {
    !matches!(app.state::<SummaryStore>().get(), SummaryState::None)
}

/// Starts writing up the meeting that just ended and shows the window.
pub fn prepare(app: &AppHandle, transcript: Vec<Turn>) {
    if transcript.len() < MIN_TURNS {
        return;
    }
    set(app, SummaryState::Preparing);
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || open_window(&handle)) {
        tracing::warn!(event = "summary_window_failed", %error);
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let settings = app.state::<SettingsStore>().get();
        // Written in the seller's language when the meeting was translated.
        let language = settings.translating().then_some(settings.my_language.as_str());
        let topics = topics::current(&app);
        let started = std::time::Instant::now();
        let result = match meeting::credentials() {
            Ok(credentials) => aura_session::summarize_meeting(&credentials, language, &transcript, &topics).await,
            Err(message) => Err(message),
        };
        tracing::info!(
            event = "summary_prepared",
            ok = result.is_ok(),
            turns = transcript.len(),
            latency_ms = started.elapsed().as_millis() as u64
        );
        set(
            &app,
            match result {
                Ok(summary) => SummaryState::Ready {
                    markdown: summary.to_markdown(),
                    summary: Box::new(summary),
                },
                Err(message) => SummaryState::Failed { message },
            },
        );
    });
}

fn set(app: &AppHandle, state: SummaryState) {
    *app.state::<SummaryStore>().0.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
    if let Err(error) = app.emit(EVENT, &state) {
        tracing::warn!(event = "summary_emit_failed", %error);
    }
}

/// An ordinary window, because the seller reads, selects and copies from it.
/// Must run on the main thread.
pub fn open_window(app: &AppHandle) {
    let shown = match app.get_webview_window(WINDOW) {
        Some(window) => window.show().and_then(|()| window.set_focus()),
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html".into()))
            .title("Aura — Meeting Summary")
            .inner_size(520.0, 660.0)
            .min_inner_size(400.0, 360.0)
            .center()
            .focused(true)
            // Best effort only, like every Aura window.
            .content_protected(true)
            .build()
            .and_then(|window| window.set_focus()),
    };
    if let Err(error) = shown {
        tracing::error!(event = "summary_window_failed", %error);
    }
}

/// Emergency hide covers this window too. Must run on the main thread.
pub fn close_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.close();
    }
}
