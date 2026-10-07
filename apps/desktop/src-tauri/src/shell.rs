//! Owns the single [`ShellState`] and pushes every change out to the windows,
//! the tray and the webviews.

use std::sync::{Mutex, MutexGuard, PoisonError};

use aura_core::shell::{ShellCommand, ShellState};
use tauri::{AppHandle, Emitter, Manager};

use crate::shortcuts::ShortcutBinding;
use crate::{tokens, topics, tray, windows};

pub const STATE_EVENT: &str = "shell://state";

#[derive(Default)]
pub struct Shell {
    state: Mutex<ShellState>,
    shortcuts: Mutex<Vec<ShortcutBinding>>,
}

// A panic while holding these locks cannot leave the plain-data contents
// half-written, so a poisoned lock is still safe to read.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shell {
    pub fn state(&self) -> ShellState {
        *lock(&self.state)
    }

    pub fn shortcuts(&self) -> Vec<ShortcutBinding> {
        lock(&self.shortcuts).clone()
    }

    pub fn set_shortcuts(&self, bindings: Vec<ShortcutBinding>) {
        *lock(&self.shortcuts) = bindings;
    }
}

pub fn dispatch(app: &AppHandle, command: ShellCommand) -> ShellState {
    let shell = app.state::<Shell>();
    let (previous, next) = {
        let mut state = lock(&shell.state);
        let previous = *state;
        state.apply(command);
        (previous, *state)
    };
    if previous != next {
        tracing::info!(event = "shell_command", ?command, hidden = next.hidden);
        publish(app, Some(previous), next);
    }
    next
}

/// Brings the windows in line with the initial state at startup.
pub fn present(app: &AppHandle) {
    publish(app, None, app.state::<Shell>().state());
}

fn publish(app: &AppHandle, previous: Option<ShellState>, next: ShellState) {
    let handle = app.clone();
    // AppKit windows may only be touched from the main thread; shortcuts and
    // IPC commands arrive on others.
    let scheduled = app.run_on_main_thread(move || {
        windows::reconcile(&handle, previous, next);
        topics::sync_visibility(&handle, next);
        if next.hidden {
            tokens::close_window(&handle);
        }
        tray::refresh(&handle);
    });
    if let Err(error) = scheduled {
        tracing::error!(event = "shell_reconcile_failed", %error);
    }
    if let Err(error) = app.emit(STATE_EVENT, next) {
        tracing::error!(event = "shell_emit_failed", %error);
    }
}
