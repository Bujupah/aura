use std::fmt;

/// Environment variable holding the developer's API token.
pub const API_TOKEN_ENV: &str = "OPENAI_API_TOKEN";

/// A bearer credential for the AI provider.
///
/// It lives only in the Rust core: it is never serialized, never logged
/// (`Debug` is redacted) and never handed to the webview. In development it
/// is a long-lived token from the environment; in production the gateway
/// issues a short-lived one and this type stays the same.
#[derive(Clone)]
pub struct ApiCredential(String);

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("the API token is empty")]
    Empty,
}

impl ApiCredential {
    pub fn new(token: impl Into<String>) -> Result<Self, CredentialError> {
        let token = token.into().trim().to_owned();
        if token.is_empty() {
            return Err(CredentialError::Empty);
        }
        Ok(Self(token))
    }

    pub fn from_env() -> Result<Self, CredentialError> {
        Self::from_env_var(API_TOKEN_ENV)
    }

    /// Reads a credential for another provider from its own variable.
    pub fn from_env_var(name: &'static str) -> Result<Self, CredentialError> {
        Self::new(std::env::var(name).map_err(|_| CredentialError::Missing(name))?)
    }

    /// The raw token, for providers that do not use a bearer header. Only
    /// for building requests inside the Rust core.
    pub fn secret(&self) -> &str {
        &self.0
    }

    /// The `Authorization` header value. Only for building requests to the
    /// provider inside the Rust core.
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for ApiCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiCredential(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_token() {
        let credential = ApiCredential::new("sk-very-secret").unwrap();
        assert!(!format!("{credential:?}").contains("secret"));
    }

    #[test]
    fn blank_tokens_are_rejected() {
        assert!(matches!(ApiCredential::new("  \n"), Err(CredentialError::Empty)));
    }
}
