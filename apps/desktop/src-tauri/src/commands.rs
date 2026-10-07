use aura_core::shell::{ShellCommand, ShellState};
use tauri::{AppHandle, State};

use crate::shell::{self, Shell};
use crate::shortcuts::ShortcutBinding;

#[tauri::command]
pub fn shell_state(shell: State<'_, Shell>) -> ShellState {
    shell.state()
}

#[tauri::command]
pub fn shell_shortcuts(shell: State<'_, Shell>) -> Vec<ShortcutBinding> {
    shell.shortcuts()
}

#[tauri::command]
pub fn shell_dispatch(app: AppHandle, command: ShellCommand) -> ShellState {
    shell::dispatch(&app, command)
}

#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}
