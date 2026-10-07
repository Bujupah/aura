//! GPT-Live over WebSocket.
//!
//! A Live session takes one continuous mono audio stream and returns
//! timestamped transcript fragments for it, the model's own (spoken) replies,
//! and delegation requests when the model wants backend work done. Aura runs
//! with client delegation, so the application — never the model — decides
//! what backend work runs and what comes back.
//!
//! Protocol reference: <https://developers.openai.com/api/docs/guides/live>

mod connection;
mod credential;
mod event;

pub use connection::{connect, LiveConnection, LiveReceiver, LiveSender};
pub use credential::{ApiCredential, CredentialError, API_TOKEN_ENV};
pub use event::{
    AudioFormat, ClientEvent, Delegation, DelegationMode, ServerError, ServerEvent, SessionConfig,
    TranscriptFragment,
};

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("could not build the connection request: {0}")]
    Request(String),
    #[error("websocket error: {0}")]
    Transport(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("the server sent an event that is not valid JSON: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("the server rejected the session: {0}")]
    Rejected(ServerError),
    #[error("the connection closed before `{0}` arrived")]
    ClosedBefore(&'static str),
    #[error("timed out after {seconds}s waiting for `{event}`")]
    Timeout { event: &'static str, seconds: u64 },
    #[error("PCM16 audio must contain whole samples, got {0} bytes")]
    PartialSample(usize),
}
