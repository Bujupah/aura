//! The provider tokens Aura runs on, and the window for entering them.
//!
//! A token typed into the app is stored in the macOS Keychain and takes
//! precedence over one from the environment. Tokens flow one way: the
//! window can submit a token, but nothing here ever sends one back to a
//! webview — it can only learn whether a token is present and from where.

use aura_live::ApiCredential;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// One Keychain item per token, with the token's name as the account.
const KEYCHAIN_SERVICE: &str = "dev.aura.desktop";
pub const WINDOW: &str = "tokens";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Openai,
    Gemini,
}

impl Provider {
    fn variable(self) -> &'static str {
        match self {
            Self::Openai => aura_live::API_TOKEN_ENV,
            Self::Gemini => aura_translate::API_TOKEN_ENV,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenSource {
    /// Entered in the app.
    Keychain,
    Environment,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenStatus {
    pub openai: TokenSource,
    pub gemini: TokenSource,
}

/// The token to use for `provider`: the one entered in the app if there is
/// one, otherwise the environment's.
pub fn resolve(provider: Provider) -> Option<ApiCredential> {
    from_keychain(provider.variable())
        .and_then(|secret| ApiCredential::new(secret).ok())
        .or_else(|| ApiCredential::from_env_var(provider.variable()).ok())
}

fn source(provider: Provider) -> TokenSource {
    if from_keychain(provider.variable()).is_some() {
        TokenSource::Keychain
    } else if ApiCredential::from_env_var(provider.variable()).is_ok() {
        TokenSource::Environment
    } else {
        TokenSource::None
    }
}

#[tauri::command]
pub fn tokens_status() -> TokenStatus {
    TokenStatus {
        openai: source(Provider::Openai),
        gemini: source(Provider::Gemini),
    }
}

/// Stores a token entered in the app. It applies from the next session.
#[tauri::command]
pub fn token_set(provider: Provider, value: String) -> Result<TokenStatus, String> {
    // Reject whitespace and control characters: a token that cannot go in a
    // header would only fail later, less clearly.
    let token = value.trim();
    if token.is_empty() {
        return Err("Enter a token first.".into());
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("That doesn't look like a token: it contains spaces or line breaks.".into());
    }
    to_keychain(provider.variable(), token)?;
    tracing::info!(event = "token_saved", ?provider);
    Ok(tokens_status())
}

/// Removes a token entered in the app, falling back to the environment's.
#[tauri::command]
pub fn token_clear(provider: Provider) -> Result<TokenStatus, String> {
    remove_from_keychain(provider.variable())?;
    tracing::info!(event = "token_removed", ?provider);
    Ok(tokens_status())
}

#[tauri::command]
pub fn tokens_open(app: AppHandle) {
    open_window(&app);
}

/// Shows the token window, creating it on first use. Unlike Aura's panels it
/// is an ordinary window, because it has to take the keyboard.
pub fn open_window(app: &AppHandle) {
    let shown = match app.get_webview_window(WINDOW) {
        Some(window) => window.show().and_then(|()| window.set_focus()),
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html".into()))
            .title("Aura — API Tokens")
            .inner_size(440.0, 284.0)
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .always_on_top(true)
            .center()
            .focused(true)
            // Best effort only, like every Aura window.
            .content_protected(true)
            .build()
            .and_then(|window| window.set_focus()),
    };
    if let Err(error) = shown {
        tracing::error!(event = "token_window_failed", %error);
    }
}

/// Emergency hide covers this window too. Must run on the main thread.
pub fn close_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.close();
    }
}

#[cfg(target_os = "macos")]
fn from_keychain(account: &str) -> Option<String> {
    let secret = security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account).ok()?;
    String::from_utf8(secret).ok().filter(|secret| !secret.trim().is_empty())
}

#[cfg(target_os = "macos")]
fn to_keychain(account: &str, secret: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, account, secret.as_bytes())
        .map_err(|error| format!("The Keychain refused to save the token: {error}"))
}

#[cfg(target_os = "macos")]
fn remove_from_keychain(account: &str) -> Result<(), String> {
    // errSecItemNotFound: there was nothing to remove, which is the goal.
    const NOT_FOUND: i32 = -25300;
    match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == NOT_FOUND => Ok(()),
        Err(error) => Err(format!("The Keychain refused to remove the token: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn from_keychain(_account: &str) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn to_keychain(_account: &str, _secret: &str) -> Result<(), String> {
    Err("Saving tokens is only supported on macOS.".into())
}

#[cfg(not(target_os = "macos"))]
fn remove_from_keychain(_account: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_must_be_one_unbroken_string() {
        assert!(token_set(Provider::Openai, "   ".into()).is_err());
        assert!(token_set(Provider::Openai, "two words".into()).is_err());
        assert!(token_set(Provider::Gemini, "line\nbreak".into()).is_err());
    }

    /// Touches the real login Keychain under a throwaway account, so it only
    /// runs when asked for: `cargo test -p aura-desktop -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(target_os = "macos")]
    fn the_keychain_round_trips_and_removal_is_idempotent() {
        const ACCOUNT: &str = "AURA_SELF_TEST_TOKEN";
        remove_from_keychain(ACCOUNT).unwrap();
        assert_eq!(from_keychain(ACCOUNT), None);
        to_keychain(ACCOUNT, "first").unwrap();
        to_keychain(ACCOUNT, "second").unwrap();
        assert_eq!(from_keychain(ACCOUNT).as_deref(), Some("second"));
        remove_from_keychain(ACCOUNT).unwrap();
        remove_from_keychain(ACCOUNT).unwrap();
        assert_eq!(from_keychain(ACCOUNT), None);
    }
}
