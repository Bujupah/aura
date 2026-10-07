//! One small window per topic of discussion.
//!
//! The note-taking agent decides which topics have a window, in which corner
//! and at what size; `aura-core` turns that into rectangles that fit the
//! screen, and this module makes the real windows match. Each is an
//! always-on-top, non-activating panel like the overlay, and emergency hide
//! covers them too.
//!
//! The seller has the last word on two things:
//!
//! * **Position.** A window the seller drags is pinned where they put it.
//!   Aura never moves it again, whatever the agent asks; it only changes its
//!   size in place. Other windows are arranged around it.
//! * **Existence.** Only the seller closes a window. Closing one removes the
//!   topic and tells the agent not to bring it back.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use aura_core::layout::{self, Rect};
use aura_core::shell::ShellState;
use aura_core::topics::{Placement, Topic};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindowBuilder,
    Window, WindowEvent,
};

use crate::shell::Shell;
use crate::{meeting, windows};

/// Keeps the right-hand zones clear of the status overlay's column.
const OVERLAY_COLUMN: f64 = 340.0 + 12.0;
const LABEL_PREFIX: &str = "topic-";
/// A window reported this far from where Aura put it was moved by hand.
const MOVED_BY_HAND: f64 = 3.0;

#[derive(Default)]
pub struct TopicWindows {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    topics: Vec<Topic>,
    /// Closed by the seller. Kept so an update already on its way from the
    /// agent cannot reopen the window.
    dismissed: HashSet<String>,
    /// Open windows and where Aura believes each one is.
    open: HashMap<String, Rect>,
    /// Windows the seller has dragged. Never repositioned.
    pinned: HashSet<String>,
    /// Finished illustrations (PNG), by topic id. Memory only.
    images: HashMap<String, Vec<u8>>,
}

impl TopicWindows {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn label(topic_id: &str) -> String {
    format!("{LABEL_PREFIX}{topic_id}")
}

#[tauri::command]
pub fn topics_current(windows: State<'_, TopicWindows>) -> Vec<Topic> {
    windows.lock().topics.clone()
}

/// The notes as they stand, for work that happens outside a webview.
pub fn current(app: &AppHandle) -> Vec<Topic> {
    app.state::<TopicWindows>().lock().topics.clone()
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

/// The seller closed a topic's window: it goes away now, and the agent is
/// told so the subject does not come back.
#[tauri::command]
pub fn topic_dismiss(app: AppHandle, id: String) {
    {
        let state = app.state::<TopicWindows>();
        let mut inner = state.lock();
        inner.dismissed.insert(id.clone());
        inner.pinned.remove(&id);
        inner.images.remove(&id);
    }
    meeting::dismiss_topic(&app, id);
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
        inner.pinned.clear();
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

/// Notices when the seller drags a topic window, and pins it there.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::Moved(position) = event else {
        return;
    };
    let Some(topic_id) = window.label().strip_prefix(LABEL_PREFIX) else {
        return;
    };
    let Ok(scale) = window.scale_factor() else {
        return;
    };
    let position = position.to_logical::<f64>(scale);

    let state = window.app_handle().state::<TopicWindows>();
    let mut inner = state.lock();
    let Some(known) = inner.open.get_mut(topic_id) else {
        return;
    };
    // Aura's own moves arrive here too, already recorded; only a position
    // Aura did not choose means the seller moved the window.
    let moved_by_hand =
        (position.x - known.x).abs() > MOVED_BY_HAND || (position.y - known.y).abs() > MOVED_BY_HAND;
    if moved_by_hand {
        known.x = position.x;
        known.y = position.y;
        if inner.pinned.insert(topic_id.to_owned()) {
            tracing::info!(event = "topic_window_pinned", topic = topic_id);
        }
    }
}

enum Change {
    Open { id: String, title: String, rect: Rect },
    Move { id: String, rect: Rect },
    Resize { id: String, width: f64, height: f64 },
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
        let shown: Vec<(String, String, Placement)> = inner
            .topics
            .iter()
            .filter(|topic| !inner.dismissed.contains(&topic.id))
            .filter_map(|topic| topic.placement.map(|placement| (topic.id.clone(), topic.title.clone(), placement)))
            .collect();

        let mut changes = Vec::new();
        let mut next_open: HashMap<String, Rect> = HashMap::with_capacity(shown.len());

        // Pinned windows first: they stay put, taking only the agent's size.
        for (id, _, placement) in &shown {
            let Some(current) = inner.open.get(id).filter(|_| inner.pinned.contains(id)) else {
                continue;
            };
            let (width, height) = layout::dimensions(placement.size);
            if current.width != width || current.height != height {
                changes.push(Change::Resize { id: id.clone(), width, height });
            }
            next_open.insert(id.clone(), Rect { width, height, ..*current });
        }

        // Everything else is arranged around them.
        let obstacles: Vec<Rect> = next_open.values().copied().collect();
        let arranged: Vec<&(String, String, Placement)> =
            shown.iter().filter(|(id, _, _)| !next_open.contains_key(id)).collect();
        let placements: Vec<Placement> = arranged.iter().map(|(_, _, placement)| *placement).collect();
        let rects = layout::arrange(area, OVERLAY_COLUMN, &placements, &obstacles);
        for ((id, title, _), rect) in arranged.into_iter().zip(rects) {
            // The agent asked for more than this screen can hold.
            let Some(rect) = rect else {
                tracing::warn!(event = "topic_window_does_not_fit", topic = %id);
                continue;
            };
            match inner.open.get(id) {
                None => changes.push(Change::Open { id: id.clone(), title: title.clone(), rect }),
                Some(current) if *current != rect => changes.push(Change::Move { id: id.clone(), rect }),
                Some(_) => {}
            }
            next_open.insert(id.clone(), rect);
        }

        for id in inner.open.keys().filter(|id| !next_open.contains_key(*id)) {
            changes.push(Change::Close { id: id.clone() });
        }
        inner.pinned.retain(|id| next_open.contains_key(id));
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
            Change::Resize { id, width, height } => on_main(app, move |app| {
                let resized = windows::window(app, &label(&id))
                    .and_then(|window| window.set_size(LogicalSize::new(width, height)));
                if let Err(error) = resized {
                    tracing::warn!(event = "topic_resize_failed", topic = %id, %error);
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
