//! Global shortcuts. Defaults live in one table so they can become
//! user-configurable without touching the handler.

use aura_core::shell::ShellCommand;
use serde::Serialize;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

use crate::shell::{self, Shell};

struct Definition {
    id: &'static str,
    label: &'static str,
    keys: &'static str,
    modifiers: Modifiers,
    code: Code,
    command: ShellCommand,
}

impl Definition {
    fn shortcut(&self) -> Shortcut {
        Shortcut::new(Some(self.modifiers), self.code)
    }
}

fn definitions() -> [Definition; 3] {
    [
        Definition {
            id: "palette",
            label: "Command palette",
            keys: "⌥Space",
            modifiers: Modifiers::ALT,
            code: Code::Space,
            command: ShellCommand::TogglePalette,
        },
        Definition {
            id: "hide",
            label: "Hide / restore all Aura windows",
            keys: "⌘⇧.",
            modifiers: Modifiers::SUPER.union(Modifiers::SHIFT),
            code: Code::Period,
            command: ShellCommand::ToggleHidden,
        },
        Definition {
            id: "overlay",
            label: "Expand / collapse overlay",
            keys: "⌥⇧Space",
            modifiers: Modifiers::ALT.union(Modifiers::SHIFT),
            code: Code::Space,
            command: ShellCommand::ToggleOverlayMode,
        },
    ]
}

/// What the UI needs to show a shortcut, including whether macOS actually
/// gave it to us (another app may already own the combination).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static str,
    pub registered: bool,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if let Some(definition) = definitions().iter().find(|d| &d.shortcut() == shortcut) {
                shell::dispatch(app, definition.command);
            }
        })
        .build()
}

pub fn register(app: &AppHandle) {
    let bindings = definitions()
        .iter()
        .map(|definition| {
            let result = app.global_shortcut().register(definition.shortcut());
            if let Err(error) = &result {
                tracing::warn!(
                    event = "shortcut_unavailable",
                    id = definition.id,
                    keys = definition.keys,
                    %error
                );
            }
            ShortcutBinding {
                id: definition.id,
                label: definition.label,
                keys: definition.keys,
                registered: result.is_ok(),
            }
        })
        .collect();
    app.state::<Shell>().set_shortcuts(bindings);
}
