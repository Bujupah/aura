//! Desktop shell state machine.
//!
//! The shell owns exactly one piece of truth: what Aura's windows should look
//! like right now. Every input (global shortcut, tray menu, palette command,
//! overlay click) is a [`ShellCommand`]; the platform layer applies it with
//! [`ShellState::apply`] and then reconciles real windows to the new state.
//! Keeping the transitions here makes them deterministic and testable without
//! a window server.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayMode {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    pub overlay_mode: OverlayMode,
    /// Emergency hide. While true no Aura window may be on screen.
    pub hidden: bool,
    /// The overlay ignores the pointer so clicks reach the app underneath.
    pub click_through: bool,
    pub palette_open: bool,
    /// Whether the per-topic note windows are shown.
    pub topics_visible: bool,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            overlay_mode: OverlayMode::Collapsed,
            hidden: false,
            click_through: false,
            palette_open: false,
            topics_visible: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShellCommand {
    ToggleOverlayMode,
    SetOverlayMode { mode: OverlayMode },
    ToggleHidden,
    ToggleClickThrough,
    TogglePalette,
    ClosePalette,
    ToggleTopics,
}

impl ShellState {
    /// Applies `command` and reports whether the state changed.
    pub fn apply(&mut self, command: ShellCommand) -> bool {
        let before = *self;
        match command {
            ShellCommand::ToggleHidden => {
                self.hidden = !self.hidden;
                if self.hidden {
                    self.palette_open = false;
                }
            }
            // Hidden is a promise to the seller that nothing appears on
            // screen, so the only command honoured while hidden is the one
            // that restores Aura.
            _ if self.hidden => {}
            ShellCommand::ToggleOverlayMode => {
                self.overlay_mode = match self.overlay_mode {
                    OverlayMode::Collapsed => OverlayMode::Expanded,
                    OverlayMode::Expanded => OverlayMode::Collapsed,
                };
            }
            ShellCommand::SetOverlayMode { mode } => self.overlay_mode = mode,
            ShellCommand::ToggleClickThrough => self.click_through = !self.click_through,
            ShellCommand::TogglePalette => self.palette_open = !self.palette_open,
            ShellCommand::ClosePalette => self.palette_open = false,
            ShellCommand::ToggleTopics => self.topics_visible = !self.topics_visible,
        }
        *self != before
    }

    pub fn overlay_visible(&self) -> bool {
        !self.hidden
    }

    pub fn palette_visible(&self) -> bool {
        !self.hidden && self.palette_open
    }

    pub fn topic_windows_visible(&self) -> bool {
        !self.hidden && self.topics_visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_after(commands: &[ShellCommand]) -> ShellState {
        let mut state = ShellState::default();
        for command in commands {
            state.apply(*command);
        }
        state
    }

    #[test]
    fn starts_collapsed_and_visible() {
        let state = ShellState::default();
        assert_eq!(state.overlay_mode, OverlayMode::Collapsed);
        assert!(state.overlay_visible());
        assert!(!state.palette_visible());
    }

    #[test]
    fn toggle_overlay_mode_round_trips() {
        let mut state = ShellState::default();
        assert!(state.apply(ShellCommand::ToggleOverlayMode));
        assert_eq!(state.overlay_mode, OverlayMode::Expanded);
        assert!(state.apply(ShellCommand::ToggleOverlayMode));
        assert_eq!(state.overlay_mode, OverlayMode::Collapsed);
    }

    #[test]
    fn setting_the_current_mode_reports_no_change() {
        let mut state = ShellState::default();
        assert!(!state.apply(ShellCommand::SetOverlayMode {
            mode: OverlayMode::Collapsed
        }));
    }

    #[test]
    fn emergency_hide_closes_the_palette_and_hides_everything() {
        let state = state_after(&[ShellCommand::TogglePalette, ShellCommand::ToggleHidden]);
        assert!(state.hidden);
        assert!(!state.palette_open);
        assert!(!state.overlay_visible());
        assert!(!state.palette_visible());
        assert!(!state.topic_windows_visible());
    }

    #[test]
    fn nothing_but_restore_is_honoured_while_hidden() {
        let hidden = state_after(&[ShellCommand::ToggleHidden]);
        for command in [
            ShellCommand::ToggleOverlayMode,
            ShellCommand::SetOverlayMode {
                mode: OverlayMode::Expanded,
            },
            ShellCommand::ToggleClickThrough,
            ShellCommand::TogglePalette,
            ShellCommand::ClosePalette,
            ShellCommand::ToggleTopics,
        ] {
            let mut state = hidden;
            assert!(!state.apply(command), "{command:?} changed hidden state");
            assert_eq!(state, hidden);
        }
    }

    #[test]
    fn restore_brings_back_the_previous_overlay() {
        let state = state_after(&[
            ShellCommand::ToggleOverlayMode,
            ShellCommand::ToggleClickThrough,
            ShellCommand::ToggleHidden,
            ShellCommand::ToggleHidden,
        ]);
        assert!(state.overlay_visible());
        assert_eq!(state.overlay_mode, OverlayMode::Expanded);
        assert!(state.click_through);
        assert!(!state.palette_open);
    }

    #[test]
    fn wire_format_is_stable() {
        let json = serde_json::to_value(ShellState::default()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "overlayMode": "collapsed",
                "hidden": false,
                "clickThrough": false,
                "paletteOpen": false,
                "topicsVisible": true
            })
        );
        let command: ShellCommand =
            serde_json::from_str(r#"{"type":"setOverlayMode","mode":"expanded"}"#).unwrap();
        assert_eq!(
            command,
            ShellCommand::SetOverlayMode {
                mode: OverlayMode::Expanded
            }
        );
    }
}
