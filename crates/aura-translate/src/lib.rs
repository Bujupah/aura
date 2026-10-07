//! Gemini Live Translate over WebSocket.
//!
//! The model behaves as an interpreter, not an assistant: it takes one
//! continuous audio stream and, without waiting for turns, returns the same
//! speech in the target language, plus text of what it heard and what it
//! said. It accepts no instructions and has no tools.
//!
//! Protocol reference: <https://ai.google.dev/gemini-api/docs/live-api/live-translate>

use std::time::Duration;

use aura_live::ApiCredential;
use base64::Engine;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Environment variable holding the developer's Gemini API token.
pub const API_TOKEN_ENV: &str = "GEMINI_API_TOKEN";

/// Mono signed 16-bit PCM in, at this rate.
pub const INPUT_SAMPLE_RATE: u32 = 16_000;
/// Mono signed 16-bit PCM out, at this rate.
pub const OUTPUT_SAMPLE_RATE: u32 = 24_000;

const ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
const MODEL: &str = "models/gemini-3.5-live-translate-preview";
const SETUP_TIMEOUT: Duration = Duration::from_secs(15);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("could not build the connection request: {0}")]
    Request(String),
    #[error("websocket error: {0}")]
    Transport(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("the server sent a message that is not valid JSON: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("the translation service refused the session: {0}")]
    Rejected(String),
    #[error("timed out after {0}s waiting for the session to start")]
    Timeout(u64),
    #[error("PCM16 audio must contain whole samples, got {0} bytes")]
    PartialSample(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateConfig {
    /// BCP-47 code of the language to translate into, such as `es`.
    pub target_language: String,
    /// What to do with speech that is already in the target language:
    /// repeat it (`true`) or stay silent (`false`).
    pub echo_target_language: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TranslateEvent {
    /// Text of the speech that was sent, with the language it was heard as.
    Heard { text: String, language: Option<String> },
    /// Text of the translation being spoken.
    Said { text: String },
    /// Translated speech: mono PCM16 at [`OUTPUT_SAMPLE_RATE`].
    Audio(Vec<u8>),
}

/// Opens a session and waits until the service is ready for audio.
pub async fn connect(
    credential: &ApiCredential,
    config: &TranslateConfig,
) -> Result<TranslateConnection, TranslateError> {
    let mut request = ENDPOINT
        .into_client_request()
        .map_err(|error| TranslateError::Request(error.to_string()))?;
    // Sent as a header, never in the URL, so the token cannot end up in a
    // logged address.
    let mut key = HeaderValue::from_str(credential.secret())
        .map_err(|_| TranslateError::Request("the API token contains invalid characters".into()))?;
    key.set_sensitive(true);
    request.headers_mut().insert("x-goog-api-key", key);

    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    let setup = json!({ "setup": {
        "model": MODEL,
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "translationConfig": {
                "targetLanguageCode": config.target_language,
                "echoTargetLanguage": config.echo_target_language,
            }
        }
    }});
    socket.send(Message::Text(setup.to_string().into())).await?;

    tokio::time::timeout(SETUP_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(message)) => match parse(&message)? {
                    Some(payload) if payload.get("setupComplete").is_some() => return Ok(()),
                    Some(payload) if payload.get("error").is_some() => {
                        return Err(TranslateError::Rejected(payload["error"].to_string()))
                    }
                    _ => {}
                },
                Some(Err(error)) => return Err(error.into()),
                None => return Err(TranslateError::Rejected("the connection closed during setup".into())),
            }
        }
    })
    .await
    .map_err(|_| TranslateError::Timeout(SETUP_TIMEOUT.as_secs()))??;

    Ok(TranslateConnection { socket })
}

pub struct TranslateConnection {
    socket: Socket,
}

impl TranslateConnection {
    pub fn split(self) -> (TranslateSender, TranslateReceiver) {
        let (sink, stream) = self.socket.split();
        (TranslateSender { sink }, TranslateReceiver { stream })
    }
}

pub struct TranslateSender {
    sink: SplitSink<Socket, Message>,
}

impl TranslateSender {
    /// Appends PCM16 audio at [`INPUT_SAMPLE_RATE`]. The service recommends
    /// 100 ms chunks.
    pub async fn append_audio(&mut self, pcm: &[u8]) -> Result<(), TranslateError> {
        if !pcm.len().is_multiple_of(2) {
            return Err(TranslateError::PartialSample(pcm.len()));
        }
        let message = json!({ "realtimeInput": { "audio": {
            "data": base64::engine::general_purpose::STANDARD.encode(pcm),
            "mimeType": format!("audio/pcm;rate={INPUT_SAMPLE_RATE}"),
        }}});
        self.sink.send(Message::Text(message.to_string().into())).await?;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), TranslateError> {
        self.sink.close().await?;
        Ok(())
    }
}

pub struct TranslateReceiver {
    stream: SplitStream<Socket>,
}

impl TranslateReceiver {
    /// The events in the next server message; one message can carry several.
    /// `None` once the connection has closed.
    pub async fn next_events(&mut self) -> Option<Result<Vec<TranslateEvent>, TranslateError>> {
        loop {
            return match self.stream.next().await? {
                Ok(Message::Close(frame)) => match frame {
                    // 1000 is a normal close; anything else carries a reason.
                    Some(frame) if u16::from(frame.code) != 1000 => {
                        Some(Err(TranslateError::Rejected(frame.reason.to_string())))
                    }
                    _ => None,
                },
                Ok(message) => match parse(&message) {
                    Ok(Some(payload)) => {
                        let events = events_in(&payload);
                        if events.is_empty() {
                            continue;
                        }
                        Some(Ok(events))
                    }
                    Ok(None) => continue,
                    Err(error) => Some(Err(error)),
                },
                Err(error) => Some(Err(error.into())),
            };
        }
    }
}

/// The service sends JSON as either text or binary frames.
fn parse(message: &Message) -> Result<Option<Value>, TranslateError> {
    match message {
        Message::Text(text) => Ok(Some(serde_json::from_str(text)?)),
        Message::Binary(bytes) => Ok(Some(serde_json::from_slice(bytes)?)),
        _ => Ok(None),
    }
}

fn events_in(payload: &Value) -> Vec<TranslateEvent> {
    let Some(content) = payload.get("serverContent") else {
        return Vec::new();
    };
    let mut events = Vec::new();
    if let Some(text) = content.pointer("/inputTranscription/text").and_then(Value::as_str) {
        if !text.is_empty() {
            events.push(TranslateEvent::Heard {
                text: text.to_owned(),
                language: content
                    .pointer("/inputTranscription/languageCode")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
    }
    if let Some(text) = content.pointer("/outputTranscription/text").and_then(Value::as_str) {
        if !text.is_empty() {
            events.push(TranslateEvent::Said { text: text.to_owned() });
        }
    }
    for part in content.pointer("/modelTurn/parts").and_then(Value::as_array).into_iter().flatten() {
        let audio = part
            .pointer("/inlineData/data")
            .and_then(Value::as_str)
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok());
        if let Some(audio) = audio.filter(|audio| !audio.is_empty()) {
            events.push(TranslateEvent::Audio(audio));
        }
    }
    events
}

/// Whether a spoken-language code such as `es-ES` is the language `target`
/// (`es`). Regional variants of one language count as the same.
pub fn is_language(code: &str, target: &str) -> bool {
    let primary = |tag: &str| tag.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    !code.is_empty() && primary(code) == primary(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_message_can_carry_heard_text_said_text_and_audio() {
        let audio = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3, 4]);
        let payload = json!({ "serverContent": {
            "inputTranscription": { "text": "Hello", "languageCode": "en-US" },
            "outputTranscription": { "text": "Hola" },
            "modelTurn": { "parts": [{ "inlineData": { "data": audio, "mimeType": "audio/pcm;rate=24000" } }] }
        }});
        assert_eq!(
            events_in(&payload),
            [
                TranslateEvent::Heard { text: "Hello".into(), language: Some("en-US".into()) },
                TranslateEvent::Said { text: "Hola".into() },
                TranslateEvent::Audio(vec![1, 2, 3, 4]),
            ]
        );
    }

    #[test]
    fn housekeeping_messages_carry_no_events() {
        assert!(events_in(&json!({ "sessionResumptionUpdate": {} })).is_empty());
        assert!(events_in(&json!({ "serverContent": { "inputTranscription": { "text": "" } } })).is_empty());
    }

    #[test]
    fn json_arrives_as_text_or_binary() {
        let text = Message::Text(r#"{"setupComplete":{}}"#.into());
        let binary = Message::Binary(br#"{"setupComplete":{}}"#.to_vec().into());
        for message in [text, binary] {
            assert!(parse(&message).unwrap().unwrap().get("setupComplete").is_some());
        }
    }

    #[test]
    fn regional_variants_are_the_same_language() {
        assert!(is_language("es-ES", "es"));
        assert!(is_language("pt-BR", "pt-PT"));
        assert!(is_language("EN", "en"));
        assert!(!is_language("en-US", "es"));
        assert!(!is_language("", "es"));
    }
}
