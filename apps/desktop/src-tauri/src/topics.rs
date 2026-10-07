//! One small window per topic of discussion, arranged by the note-taking
//! agent.
//!
//! The agent states which topics have a window, in what order, in which
//! corner and at what size; `aura-core` turns that into rectangles that fit
//! the screen. This module makes the real windows match: it opens, closes,
//! moves and resizes them. Each is an always-on-top, non-activating panel
//! like the overlay, and emergency hide covers them too.
//!
//! The seller keeps two overrides. A window they close stays closed for the
//! session, and a window they drag stays where they put it until the agent
//! next gives that window a different place.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use aura_core::layout::{self, Rect};
use aura_core::shell::ShellState;
use aura_core::topics::{Placement, Topic};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindowBuilder,
};

use crate::shell::Shell;
use crate::windows;

/// Keeps the right-hand zones clear of the status overlay's column.
const OVERLAY_COLUMN: f64 = 340.0 + 12.0;

#[derive(Default)]
pub struct TopicWindows {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    topics: Vec<Topic>,
    /// Closed by the seller; not reopened during this session.
    dismissed: HashSet<String>,
    /// Open windows and the rectangle the agent last assigned to each.
    open: HashMap<String, Rect>,
    /// Finished illustrations (PNG), by topic id. Memory only.
    images: HashMap<String, Vec<u8>>,
}

impl TopicWindows {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn label(topic_id: &str) -> String {
    format!("topic-{topic_id}")
}

#[tauri::command]
pub fn topics_current(windows: State<'_, TopicWindows>) -> Vec<Topic> {
    windows.lock().topics.clone()
}

/// The topic's illustration as PNG bytes, or an empty body if there is none.
/// Sent as raw bytes rather than JSON: a picture is around a megabyte.
#[tauri::command]
pub fn topic_image(windows: State<'_, TopicWindows>, id: String) -> tauri::ipc::Response {
    tauri::ipc::Response::new(windows.lock().images.get(&id).cloned().unwrap_or_default())
}

pub fn store_image(app: &AppHandle, topic_id: String, png: Vec<u8>) {
    app.state::<TopicWindows>().lock().images.insert(topic_id, png);
}

#[tauri::command]
pub fn topic_dismiss(app: AppHandle, id: String) {
    app.state::<TopicWindows>().lock().dismissed.insert(id);
    // The remaining windows close the gap.
    apply(&app);
}

/// Stores the agent's latest arrangement and makes the windows match it.
pub fn update(app: &AppHandle, topics: Vec<Topic>) {
    app.state::<TopicWindows>().lock().topics = topics;
    apply(app);
}

/// Closes every topic window and forgets the previous session's notes.
pub fn reset(app: &AppHandle) {
    {
        let state = app.state::<TopicWindows>();
        let mut inner = state.lock();
        inner.topics.clear();
        inner.dismissed.clear();
        inner.images.clear();
        // `open` is left as it is: `apply` closes whatever it still lists.
    }
    apply(app);
}

/// Must run on the main thread.
pub fn sync_visibility(app: &AppHandle, shell: ShellState) {
    let labels: Vec<String> = app.state::<TopicWindows>().lock().open.keys().map(|id| label(id)).collect();
    for label in labels {
        windows::set_visible(app, &label, shell.topic_windows_visible(), false);
    }
}

enum Change {
    Open { id: String, title: String, rect: Rect },
    Move { id: String, rect: Rect },
    Close { id: String },
}

fn apply(app: &AppHandle) {
    let area = match screen_area(app) {
        Ok(Some(area)) => area,
        Ok(None) => return,
        Err(error) => return tracing::error!(event = "topic_layout_failed", %error),
    };

    let changes: Vec<Change> = {
        let state = app.state::<TopicWindows>();
        let mut inner = state.lock();
        let shown: Vec<(&Topic, Placement)> = inner
            .topics
            .iter()
            .filter(|topic| !inner.dismissed.contains(&topic.id))
            .filter_map(|topic| topic.placement.map(|placement| (topic, placement)))
            .collect();
        let placements: Vec<Placement> = shown.iter().map(|(_, placement)| *placement).collect();
        let rects = layout::arrange(area, OVERLAY_COLUMN, &placements);

        let mut changes = Vec::new();
        let mut next_open = HashMap::with_capacity(shown.len());
        for ((topic, _), rect) in shown.iter().zip(rects) {
            // The agent asked for more than this screen can hold.
            let Some(rect) = rect else {
                tracing::warn!(event = "topic_window_does_not_fit", topic = %topic.id);
                continue;
            };
            match inner.open.get(&topic.id) {
                None => changes.push(Change::Open {
                    id: topic.id.clone(),
                    title: topic.title.clone(),
                    rect,
                }),
                // Only a new assignment moves a window, so one the seller
                // dragged is otherwise left alone.
                Some(assigned) if *assigned != rect => changes.push(Change::Move {
                    id: topic.id.clone(),
                    rect,
                }),
                Some(_) => {}
            }
            next_open.insert(topic.id.clone(), rect);
        }
        for id in inner.open.keys().filter(|id| !next_open.contains_key(*id)) {
            changes.push(Change::Close { id: id.clone() });
        }
        inner.open = next_open;
        changes
    };

    for change in changes {
        match change {
            Change::Open { id, title, rect } => {
                if let Err(error) = open(app, &id, &title, rect) {
                    tracing::error!(event = "topic_window_failed", topic = %id, %error);
                }
            }
            Change::Move { id, rect } => on_main(app, move |app| {
                let moved = windows::window(app, &label(&id)).and_then(|window| {
                    window.set_size(LogicalSize::new(rect.width, rect.height))?;
                    window.set_position(LogicalPosition::new(rect.x, rect.y))
                });
                if let Err(error) = moved {
                    tracing::warn!(event = "topic_move_failed", topic = %id, %error);
                }
            }),
            Change::Close { id } => on_main(app, move |app| windows::close(app, &label(&id))),
        }
    }
}

fn open(app: &AppHandle, id: &str, title: &str, rect: Rect) -> tauri::Result<()> {
    let label = label(id);
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(rect.width, rect.height)
        .position(rect.x, rect.y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .accept_first_mouse(true)
        // Best effort only: see docs/architecture/macos.md §5.
        .content_protected(true)
        .build()?;

    on_main(app, move |app| {
        if let Err(error) = windows::adopt_topic(app, &label) {
            return tracing::error!(event = "topic_panel_failed", %label, %error);
        }
        // Honour an emergency hide that is in force right now.
        let visible = app.state::<Shell>().state().topic_windows_visible();
        windows::set_visible(app, &label, visible, false);
    });
    Ok(())
}

fn screen_area(app: &AppHandle) -> tauri::Result<Option<Rect>> {
    let Some(monitor) = windows::window(app, windows::OVERLAY)?.primary_monitor()? else {
        return Ok(None);
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let origin = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    Ok(Some(Rect {
        x: origin.x,
        y: origin.y,
        width: size.width,
        height: size.height,
    }))
}

/// AppKit windows may only be touched from the main thread.
fn on_main(app: &AppHandle, work: impl FnOnce(&AppHandle) + Send + 'static) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || work(&handle)) {
        tracing::warn!(event = "topic_window_task_failed", %error);
    }
}
