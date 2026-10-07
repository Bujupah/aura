use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One format applies to both directions and is fixed for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AudioFormat {
    #[serde(rename = "type")]
    kind: &'static str,
    rate: u32,
}

impl AudioFormat {
    /// Mono signed 16-bit little-endian PCM at 24 kHz, the API default.
    pub const PCM16_24K: Self = Self {
        kind: "audio/pcm",
        rate: 24_000,
    };

    pub fn sample_rate(self) -> u32 {
        self.rate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DelegationMode {
    /// The application runs the backend and decides what is returned.
    Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionConfig {
    pub model: String,
    pub instructions: String,
    pub audio: AudioConfig,
    pub delegation: DelegationMode,
    /// Never ask the provider to keep the recording unless policy says so.
    pub store: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AudioConfig {
    pub format: AudioFormat,
}

impl SessionConfig {
    pub fn new(model: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: instructions.into(),
            audio: AudioConfig {
                format: AudioFormat::PCM16_24K,
            },
            delegation: DelegationMode::Client,
            store: false,
        }
    }
}

/// Commands Aura sends. `event_id` lets a rejection or acknowledgment be
/// matched to the command through the server's `client_event_id`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    #[serde(rename = "session.start")]
    SessionStart { session: SessionConfig },
    /// Base64 PCM. Unacknowledged by the server.
    #[serde(rename = "session.input_audio.append")]
    InputAudioAppend { audio: String },
    /// Facts the model may use later without saying them now.
    #[serde(rename = "session.thinking.append")]
    ThinkingAppend {
        event_id: String,
        delegation_id: Option<String>,
        content: String,
    },
    /// A result for the model to relay; it may paraphrase.
    #[serde(rename = "session.commentary.append")]
    CommentaryAppend {
        event_id: String,
        delegation_id: Option<String>,
        content: String,
    },
    /// Trusted application instructions.
    #[serde(rename = "session.instructions.append")]
    InstructionsAppend {
        event_id: String,
        delegation_id: Option<String>,
        content: String,
    },
    #[serde(rename = "session.input_audio.mute")]
    InputAudioMute { event_id: String },
    #[serde(rename = "session.input_audio.unmute")]
    InputAudioUnmute { event_id: String },
    #[serde(rename = "session.close")]
    SessionClose,
}

/// A piece of transcript and where it falls on the session timeline, in
/// milliseconds from session start. Fragments are not turns: the API marks no
/// turn boundaries, so grouping is the caller's job.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TranscriptFragment {
    pub delta: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Delegation {
    pub id: String,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, thiserror::Error)]
#[error("{}{}", .code.as_deref().map(|c| format!("[{c}] ")).unwrap_or_default(), .message)]
pub struct ServerError {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub client_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    SessionStarted { session_id: String },
    /// What the audio we sent says.
    InputTranscript(TranscriptFragment),
    /// What the model is saying in reply.
    OutputTranscript(TranscriptFragment),
    /// The model's speech as base64 audio. Aura is visual-first and normally
    /// drops this without decoding.
    OutputAudio { delta: String },
    /// The model wants backend work. Carries no task text by design; the
    /// application works out the request from the transcript and its state.
    DelegationCreated { offset_ms: u64, delegation: Delegation },
    Error(ServerError),
    /// Final event of a session, with the authoritative usage for billing.
    SessionClosed { usage: Value },
    /// Acknowledgments and anything this client does not model yet.
    Other { kind: String, payload: Value },
}

impl ServerEvent {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        let payload: Value = serde_json::from_str(text)?;
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(match kind.as_str() {
            "session.started" => Self::SessionStarted {
                session_id: payload
                    .pointer("/session/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            "session.input_transcript.delta" => {
                Self::InputTranscript(serde_json::from_value(payload)?)
            }
            "session.output_transcript.delta" => {
                Self::OutputTranscript(serde_json::from_value(payload)?)
            }
            "session.output_audio.delta" => Self::OutputAudio {
                delta: payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            "session.delegation.created" => {
                #[derive(Deserialize)]
                struct Created {
                    offset_ms: u64,
                    delegation: Delegation,
                }
                let created: Created = serde_json::from_value(payload)?;
                Self::DelegationCreated {
                    offset_ms: created.offset_ms,
                    delegation: created.delegation,
                }
            }
            "error" => {
                let mut error: ServerError = match payload.get("error") {
                    Some(error) => serde_json::from_value(error.clone())?,
                    None => serde_json::from_value(payload.clone())?,
                };
                if error.client_event_id.is_none() {
                    error.client_event_id = payload
                        .get("client_event_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                Self::Error(error)
            }
            "session.closed" => Self::SessionClosed {
                usage: payload.get("usage").cloned().unwrap_or(Value::Null),
            },
            _ => Self::Other { kind, payload },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_start_matches_the_documented_shape() {
        let event = ClientEvent::SessionStart {
            session: SessionConfig::new("gpt-live-1", "Be brief."),
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "type": "session.start",
                "session": {
                    "model": "gpt-live-1",
                    "instructions": "Be brief.",
                    "audio": { "format": { "type": "audio/pcm", "rate": 24000 } },
                    "delegation": { "type": "client" },
                    "store": false
                }
            })
        );
    }

    #[test]
    fn session_wide_context_serializes_a_null_delegation_id() {
        let event = ClientEvent::ThinkingAppend {
            event_id: "ctx_1".into(),
            delegation_id: None,
            content: "fact".into(),
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "type": "session.thinking.append",
                "event_id": "ctx_1",
                "delegation_id": null,
                "content": "fact"
            })
        );
    }

    #[test]
    fn parses_the_documented_transcript_delta() {
        let event = ServerEvent::parse(
            r#"{"type":"session.input_transcript.delta","event_id":"event_transcript_1",
                "delta":"What is","start_ms":1000,"end_ms":1200}"#,
        )
        .unwrap();
        assert_eq!(
            event,
            ServerEvent::InputTranscript(TranscriptFragment {
                delta: "What is".into(),
                start_ms: 1000,
                end_ms: 1200
            })
        );
    }

    #[test]
    fn parses_the_documented_delegation() {
        let event = ServerEvent::parse(
            r#"{"type":"session.delegation.created","event_id":"event_delegation","offset_ms":1000,
                "delegation":{"id":"item_9tA2","type":"delegation","target":"client"}}"#,
        )
        .unwrap();
        assert_eq!(
            event,
            ServerEvent::DelegationCreated {
                offset_ms: 1000,
                delegation: Delegation {
                    id: "item_9tA2".into(),
                    target: Some("client".into())
                }
            }
        );
    }

    #[test]
    fn unknown_events_are_kept_not_dropped() {
        let event = ServerEvent::parse(r#"{"type":"session.thinking.appended","x":1}"#).unwrap();
        assert!(matches!(event, ServerEvent::Other { kind, .. } if kind == "session.thinking.appended"));
    }

    #[test]
    fn errors_surface_code_message_and_the_command_they_refer_to() {
        let event = ServerEvent::parse(
            r#"{"type":"error","error":{"code":"invalid_request","message":"nope","client_event_id":"e1"}}"#,
        )
        .unwrap();
        let ServerEvent::Error(error) = event else {
            panic!("expected an error event");
        };
        assert_eq!(error.to_string(), "[invalid_request] nope");
        assert_eq!(error.client_event_id.as_deref(), Some("e1"));
    }
}
