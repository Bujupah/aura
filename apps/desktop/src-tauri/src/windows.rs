//! Turns [`ShellState`] into real windows.
//!
//! On macOS both windows are converted to non-activating `NSPanel`s: they
//! float above full-screen meeting apps on every Space, and showing or
//! clicking them never activates Aura, so the meeting app keeps focus.

use aura_core::shell::{OverlayMode, ShellState};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewWindow};

pub const OVERLAY: &str = "overlay";
pub const PALETTE: &str = "palette";

const OVERLAY_WIDTH: f64 = 340.0;
const OVERLAY_COLLAPSED_HEIGHT: f64 = 38.0;
const OVERLAY_EXPANDED_HEIGHT: f64 = 300.0;
const SCREEN_MARGIN: f64 = 24.0;
/// The palette sits in the upper part of the screen, like Spotlight.
const PALETTE_TOP_FRACTION: f64 = 0.22;

#[cfg(target_os = "macos")]
mod panel {
    use tauri::AppHandle;
    use tauri_nspanel::{
        tauri_panel, CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask,
        WebviewWindowExt,
    };

    tauri_panel! {
        // The overlay never takes keyboard focus; it is read and clicked only.
        panel!(OverlayPanel {
            config: {
                can_become_key_window: false,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
        // The palette takes keys without activating the app, like Spotlight.
        panel!(PalettePanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
        // Topic notes are glanced at, never typed into.
        panel!(TopicPanel {
            config: {
                can_become_key_window: false,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }

    pub fn init(app: &AppHandle) -> tauri::Result<()> {
        let overlay = super::window(app, super::OVERLAY)?.to_panel::<OverlayPanel>()?;
        let palette = super::window(app, super::PALETTE)?.to_panel::<PalettePanel>()?;
        for panel in [overlay, palette] {
            configure(&panel);
        }
        Ok(())
    }

    pub fn adopt_topic(app: &AppHandle, label: &str) -> tauri::Result<()> {
        configure(&super::window(app, label)?.to_panel::<TopicPanel>()?);
        Ok(())
    }

    pub fn close(app: &AppHandle, label: &str) {
        // `to_window` turns the panel back into an ordinary window and takes
        // it out of the panel registry itself. Closing it while it is still
        // a panel raises an Objective-C exception.
        let window = match app.get_webview_panel(label) {
            Ok(panel) => panel.to_window(),
            Err(_) => super::window(app, label).ok(),
        };
        match window.map(|window| window.close()) {
            Some(Ok(())) => {}
            Some(Err(error)) => tracing::warn!(event = "window_close_failed", label, %error),
            None => tracing::warn!(event = "window_close_failed", label, error = "no window to close"),
        }
    }

    /// Floats above everything on every Space, including other apps'
    /// full-screen Spaces, and never activates Aura.
    fn configure(panel: &PanelHandle<tauri::Wry>) {
        panel.set_level(PanelLevel::Status.value());
        panel.set_hides_on_deactivate(false);
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .ignores_cycle()
                .into(),
        );
        if let Err(error) = panel.add_style_mask(StyleMask::empty().nonactivating_panel().into()) {
            tracing::error!(event = "panel_nonactivating_failed", label = panel.label(), %error);
        }
    }

    pub fn set_visible(app: &AppHandle, label: &str, visible: bool, take_keys: bool) {
        let Ok(panel) = app.get_webview_panel(label) else {
            tracing::error!(event = "panel_missing", label);
            return;
        };
        match (visible, take_keys) {
            (true, true) => panel.show_and_make_key(),
            (true, false) => panel.show(),
            (false, _) => panel.hide(),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod panel {
    use tauri::AppHandle;

    pub fn init(_app: &AppHandle) -> tauri::Result<()> {
        Ok(())
    }

    pub fn adopt_topic(_app: &AppHandle, _label: &str) -> tauri::Result<()> {
        Ok(())
    }

    pub fn close(app: &AppHandle, label: &str) {
        if let Ok(window) = super::window(app, label) {
            let _ = window.close();
        }
    }

    pub fn set_visible(app: &AppHandle, label: &str, visible: bool, take_keys: bool) {
        let Ok(window) = super::window(app, label) else {
            return;
        };
        let result = if visible { window.show() } else { window.hide() };
        if let Err(error) = result.and_then(|()| if visible && take_keys { window.set_focus() } else { Ok(()) }) {
            tracing::error!(event = "window_visibility_failed", label, %error);
        }
    }
}

pub use panel::{adopt_topic, close, set_visible};

pub fn window(app: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    app.get_webview_window(label)
        .ok_or(tauri::Error::WindowNotFound)
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    panel::init(app)?;
    place_overlay_top_right(&window(app, OVERLAY)?)
}

fn place_overlay_top_right(overlay: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = overlay.primary_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let origin = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    overlay.set_position(LogicalPosition::new(
        origin.x + size.width - OVERLAY_WIDTH - SCREEN_MARGIN,
        origin.y + SCREEN_MARGIN,
    ))
}

/// Must run on the main thread. `previous` is `None` at startup.
pub fn reconcile(app: &AppHandle, previous: Option<ShellState>, next: ShellState) {
    if let Err(error) = reconcile_overlay(app, previous, next) {
        tracing::error!(event = "overlay_reconcile_failed", %error);
    }

    let was_open = previous.is_some_and(|state| state.palette_visible());
    let is_open = next.palette_visible();
    if is_open && !was_open {
        if let Err(error) = place_palette(app) {
            tracing::warn!(event = "palette_placement_failed", %error);
        }
        panel::set_visible(app, PALETTE, true, true);
    } else if !is_open && (was_open || previous.is_none()) {
        panel::set_visible(app, PALETTE, false, false);
    }
}

fn reconcile_overlay(
    app: &AppHandle,
    previous: Option<ShellState>,
    next: ShellState,
) -> tauri::Result<()> {
    let overlay = window(app, OVERLAY)?;
    if previous.map(|state| state.overlay_mode) != Some(next.overlay_mode) {
        let height = match next.overlay_mode {
            OverlayMode::Collapsed => OVERLAY_COLLAPSED_HEIGHT,
            OverlayMode::Expanded => OVERLAY_EXPANDED_HEIGHT,
        };
        overlay.set_size(LogicalSize::new(OVERLAY_WIDTH, height))?;
    }
    if previous.map(|state| state.click_through) != Some(next.click_through) {
        overlay.set_ignore_cursor_events(next.click_through)?;
    }
    if previous.map(|state| state.overlay_visible()) != Some(next.overlay_visible()) {
        panel::set_visible(app, OVERLAY, next.overlay_visible(), false);
    }
    Ok(())
}

/// Centres the palette on whichever screen the pointer is on.
fn place_palette(app: &AppHandle) -> tauri::Result<()> {
    let palette = window(app, PALETTE)?;
    let cursor = app.cursor_position()?;
    let monitor = match app.monitor_from_point(cursor.x, cursor.y)? {
        Some(monitor) => monitor,
        None => match palette.primary_monitor()? {
            Some(monitor) => monitor,
            None => return Ok(()),
        },
    };
    let scale = monitor.scale_factor();
    let origin = monitor.position().to_logical::<f64>(scale);
    let screen = monitor.size().to_logical::<f64>(scale);
    let size = palette.outer_size()?.to_logical::<f64>(scale);
    palette.set_position(LogicalPosition::new(
        origin.x + (screen.width - size.width) / 2.0,
        origin.y + screen.height * PALETTE_TOP_FRACTION,
    ))
}
