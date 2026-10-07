//! A listening session.
//!
//! ```text
//! microphone ──► pacer ──► realtime session ──► turn assembler ─┐
//!                                                               ├─► SessionEvent
//! system audio ─► pacer ──► realtime session ──► turn assembler ─┘
//! ```
//!
//! The two sources never share a buffer or a model session: who is speaking
//! is decided by which device the audio came from. Audio is held only in the
//! pacers' bounded memory buffers and is never written to disk.

mod input;
mod prompt;
mod stream;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use aura_core::advice::{Advice, Answer, Gaps};
use aura_core::summary::MeetingSummary;
use aura_core::topics::Topic;
use aura_core::transcript::{Speaker, Turn};
use aura_intel::{
    track_advice, track_topics, Abilities, AdvisorRequest, AdvisorSetup, AdvisorUpdate, Control,
    Earlier, Illustrator, ResponsesClient, Update,
};
use aura_live::LiveError;
use aura_translate::{TranslateConfig, TranslateError};
use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::input::{Input, Levels};
use crate::stream::{Engine, SpeakerStream, SpeechSink, TurnText};

pub use crate::stream::Credentials;
pub use aura_intel::Ask;

/// Where a session's audio comes from.
#[derive(Debug, Clone)]
pub enum AudioInput {
    /// The microphone and system audio of this Mac.
    Capture,
    /// Raw PCM16 / 24 kHz mono files played at real-time pace. For
    /// development and tests: no microphone, no permissions, repeatable.
    Fixtures { seller: PathBuf, customer: PathBuf },
}

/// Choices the seller, or organizational policy, makes for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    /// Lets the note-taking agent search the web. Search queries leave this
    /// Mac for a search provider, so this is the seller's call.
    pub web_access: bool,
    /// Lets the note-taking agent commission images from the illustration
    /// sub-agent. Each image is a separate, slower and costlier model call.
    pub illustrations: bool,
    /// Live translation between the seller's language and the meeting's.
    pub translation: Option<Translation>,
    /// A saved meeting to continue. Its turns and notes become the starting
    /// point, and new turns carry on its numbering and its clock.
    pub resume: Option<Resume>,
}

/// The state of a saved meeting that is being continued.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resume {
    pub turns: Vec<Turn>,
    pub topics: Vec<Topic>,
}

/// A continued meeting picks its clock up this long after its last turn.
const RESUME_GAP_MS: u64 = 2_000;

/// How the meeting should be interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    /// BCP-47 code of the language the seller speaks and reads.
    pub my_language: String,
    /// BCP-47 code of the language the meeting is held in.
    pub meeting_language: String,
    pub incoming: Incoming,
    /// Translate the seller's speech into the meeting's language and play it
    /// into this output device — a virtual microphone the meeting app uses.
    /// `None` leaves the seller's speech untranslated.
    pub outgoing_device_uid: Option<String>,
}

/// What to do with what the other participants say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming {
    /// Leave it alone; transcribe it as spoken.
    Off,
    /// Show it translated.
    Text,
    /// Show it translated and speak the translation to the seller.
    Voice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureReason {
    MicrophoneDenied,
    ScreenRecordingDenied,
    Capture,
    /// The realtime service could not be reached or dropped the session.
    Service,
    Credential,
    /// Translated speech has nowhere to go.
    AudioOutput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ListeningState {
    Idle,
    Starting,
    Listening,
    Failed { reason: FailureReason, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEvent {
    State { state: ListeningState },
    /// Meter levels, 0.0–1.0, about ten times a second.
    Levels { seller: f32, customer: f32 },
    /// A turn was created, extended or finalized. Match on `turn.id`.
    Turn { turn: Turn },
    /// The complete current set of topic notes.
    Topics { topics: Vec<Topic> },
    /// A finished illustration for a topic. Sent before the `Topics` event
    /// that marks it ready. The bytes stay in the core; they are not part of
    /// the serialized event.
    TopicImage {
        topic_id: String,
        #[serde(skip)]
        png: Vec<u8>,
    },
    /// The one next move to show the seller, or `None` to show nothing.
    Advice { advice: Option<Advice> },
    /// The answer to "what are we missing?", or `None` if it failed.
    Gaps { gaps: Option<Gaps> },
    /// The reply to a palette request, or `None` if it failed.
    Answer { answer: Option<Answer> },
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("could not read audio fixture {path}: {source}")]
    Fixture { path: PathBuf, source: std::io::Error },
    #[error("audio capture is already running")]
    CaptureBusy,
    #[error(transparent)]
    Live(#[from] LiveError),
    #[error(transparent)]
    Translate(#[from] TranslateError),
    #[error("Translation needs a Gemini token. Add one from the menu bar: API Tokens…")]
    NoTranslationCredential,
    #[error("the audio output for translated speech could not be opened")]
    AudioOutput,
}

impl SessionError {
    pub fn reason(&self) -> FailureReason {
        match self {
            Self::Fixture { .. } | Self::CaptureBusy => FailureReason::Capture,
            Self::Live(_) | Self::Translate(_) => FailureReason::Service,
            Self::NoTranslationCredential => FailureReason::Credential,
            Self::AudioOutput => FailureReason::AudioOutput,
        }
    }
}

/// What the provider billed for, summed over both sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub seconds: f64,
    /// False if either session closed without reporting final usage.
    pub confirmed: bool,
}

/// A small, fast model for everything that must keep up with the meeting:
/// topic notes and the next-move suggestion.
const NOTES_MODEL: &str = "gpt-5.6-luna";
/// A stronger model for the work that reviews a whole meeting: "what are we
/// missing?" and the summary at the end.
const REVIEW_MODEL: &str = "gpt-6-astra";

/// What a session leaves behind when it stops.
pub struct SessionEnd {
    pub usage: Usage,
    /// Every finalized turn, in the order it was finalized. Held in memory
    /// only, for the end-of-meeting summary.
    pub transcript: Vec<Turn>,
}
const LEVEL_INTERVAL: Duration = Duration::from_millis(100);

pub struct MeetingSession {
    stop: watch::Sender<bool>,
    streams: Vec<JoinHandle<Option<f64>>>,
    meters: JoinHandle<()>,
    notes: JoinHandle<()>,
    notes_control: mpsc::UnboundedSender<Control>,
    advisor: JoinHandle<()>,
    advisor_requests: mpsc::UnboundedSender<AdvisorRequest>,
    fan_out: JoinHandle<()>,
    transcript: Arc<Mutex<Vec<Turn>>>,
    input: Input,
}

impl MeetingSession {
    /// Connects both realtime sessions, then starts audio. Events arrive on
    /// `events` until [`stop`](Self::stop) or a failure; a failure is
    /// reported as [`ListeningState::Failed`] and ends the session's streams.
    pub async fn start(
        credentials: &Credentials,
        audio: AudioInput,
        options: SessionOptions,
        events: mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<Self, SessionError> {
        let (seller_engine, customer_engine) = engines(options.translation.as_ref())?;
        let resume = options.resume.clone().unwrap_or_default();
        let seller = SpeakerStream::new(Speaker::Seller, seller_engine, &resume.turns);
        let customer = SpeakerStream::new(Speaker::Customer, customer_engine, &resume.turns);

        // Connect before opening the microphone: if the service is down the
        // user is never recorded for nothing.
        let (seller_connection, customer_connection) = tokio::try_join!(
            seller.connect(credentials),
            customer.connect(credentials),
        )?;

        let levels = Arc::new(Levels::default());
        let (stop, stopped) = watch::channel(false);
        let input = Input::start(
            audio,
            seller.pacer(),
            customer.pacer(),
            seller.sample_rate(),
            customer.sample_rate(),
            levels.clone(),
            events.clone(),
            stopped.clone(),
        )
        .await?;

        // Finalized turns feed the note-taker and the advisor, each on its
        // own channel so a slow model call can never hold up the transcript,
        // and are kept for the end-of-meeting summary.
        let (final_turns, mut finalized) = mpsc::unbounded_channel::<Turn>();
        let (turns_to_notes, turns_for_notes) = mpsc::unbounded_channel();
        let (turns_to_advisor, turns_for_advisor) = mpsc::unbounded_channel();
        let transcript = Arc::new(Mutex::new(resume.turns.clone()));
        let fan_out = {
            let transcript = transcript.clone();
            tokio::spawn(async move {
                while let Some(turn) = finalized.recv().await {
                    transcript.lock().unwrap_or_else(PoisonError::into_inner).push(turn.clone());
                    let _ = turns_to_notes.send(turn.clone());
                    let _ = turns_to_advisor.send(turn);
                }
            })
        };

        let language = options.translation.as_ref().map(|translation| translation.my_language.clone());
        let (notes_control, control_for_notes) = mpsc::unbounded_channel();
        let (advisor_requests, requests_for_advisor) = mpsc::unbounded_channel();
        let (topics_now, topics_seen) = watch::channel(Vec::new());
        let clients = ResponsesClient::new(credentials.openai.clone(), NOTES_MODEL)
            .and_then(|fast| Ok((fast, ResponsesClient::new(credentials.openai.clone(), REVIEW_MODEL)?)));
        let (notes, advisor) = match clients {
            Ok((fast, deep)) => {
                let abilities = Abilities {
                    web_search: options.web_access,
                    illustrator: options.illustrations.then(|| Illustrator::new(fast.clone())),
                };
                let notes = {
                    let events = events.clone();
                    let earlier = Earlier {
                        topics: resume.topics.clone(),
                        turns: resume.turns.clone(),
                    };
                    tokio::spawn(track_topics(fast.clone(), abilities, earlier, turns_for_notes, control_for_notes, move |update| {
                        let _ = events.send(match update {
                            Update::Topics(topics) => {
                                // The advisor reads the notes as they stand.
                                let _ = topics_now.send(topics.clone());
                                SessionEvent::Topics { topics }
                            }
                            Update::Image { topic_id, png } => SessionEvent::TopicImage { topic_id, png },
                        });
                    }))
                };
                let advisor = {
                    let events = events.clone();
                    let setup = AdvisorSetup {
                        fast,
                        deep,
                        language,
                        earlier: resume.turns.clone(),
                    };
                    tokio::spawn(track_advice(
                        setup,
                        turns_for_advisor,
                        topics_seen,
                        requests_for_advisor,
                        move |update| {
                            let _ = events.send(match update {
                                AdvisorUpdate::Advice(advice) => SessionEvent::Advice { advice },
                                AdvisorUpdate::Gaps(gaps) => SessionEvent::Gaps { gaps },
                                AdvisorUpdate::Answer(answer) => SessionEvent::Answer { answer },
                            });
                        },
                    ))
                };
                (notes, advisor)
            }
            Err(error) => {
                tracing::error!(event = "intelligence_unavailable", %error);
                (tokio::spawn(async {}), tokio::spawn(async {}))
            }
        };

        // One clock for both streams, so their turns interleave correctly. A
        // continued meeting's clock is set back so that it reads on from
        // where the saved one ended.
        let already_elapsed = resume
            .turns
            .iter()
            .map(|turn| turn.end_ms)
            .max()
            .map_or(Duration::ZERO, |last| Duration::from_millis(last + RESUME_GAP_MS));
        let now = Instant::now();
        let clock = now.checked_sub(already_elapsed).unwrap_or(now);
        let streams = vec![
            tokio::spawn(seller.run(seller_connection, clock, events.clone(), final_turns.clone(), stopped.clone())),
            tokio::spawn(customer.run(customer_connection, clock, events.clone(), final_turns, stopped.clone())),
        ];
        let meters = tokio::spawn(report_levels(levels, events.clone(), stopped));

        let _ = events.send(SessionEvent::State {
            state: ListeningState::Listening,
        });
        Ok(Self {
            stop,
            streams,
            meters,
            notes,
            notes_control,
            advisor,
            advisor_requests,
            fan_out,
            transcript,
            input,
        })
    }

    /// Tells the note-taking agent the seller closed a topic's window. The
    /// topic is removed and the agent is told not to bring it back.
    pub fn dismiss_topic(&self, topic_id: String) {
        let _ = self.notes_control.send(Control::Dismiss { topic_id });
    }

    /// Asks the advisor what discovery has not yet established. The answer
    /// arrives as [`SessionEvent::Gaps`].
    pub fn ask_whats_missing(&self) {
        let _ = self.advisor_requests.send(AdvisorRequest::WhatsMissing);
    }

    /// Asks the advisor for one of the palette's on-request answers. The
    /// reply arrives as [`SessionEvent::Answer`].
    pub fn ask(&self, ask: Ask, question: Option<String>) {
        let _ = self.advisor_requests.send(AdvisorRequest::Ask { ask, question });
    }

    /// Stops audio first, then closes both realtime sessions and waits for
    /// their final usage.
    pub async fn stop(self) -> SessionEnd {
        let _ = self.stop.send(true);
        self.input.stop().await;
        let _ = self.meters.await;

        let mut usage = Usage {
            seconds: 0.0,
            confirmed: true,
        };
        for stream in self.streams {
            match stream.await {
                Ok(Some(seconds)) => usage.seconds += seconds,
                _ => usage.confirmed = false,
            }
        }
        // The streams held the only senders of final turns, so everything
        // downstream finishes what it has and ends. The advisor's last
        // thought is no longer wanted.
        let _ = self.fan_out.await;
        self.advisor.abort();
        let _ = self.notes.await;
        let transcript = std::mem::take(&mut *self.transcript.lock().unwrap_or_else(PoisonError::into_inner));
        SessionEnd { usage, transcript }
    }
}

async fn report_levels(
    levels: Arc<Levels>,
    events: mpsc::UnboundedSender<SessionEvent>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(LEVEL_INTERVAL);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let (seller, customer) = levels.take();
                if events.send(SessionEvent::Levels { seller, customer }).is_err() {
                    return;
                }
            }
            _ = stopped.changed() => return,
        }
    }
}

/// Chooses each stream's engine from the translation settings.
///
/// The seller's turns always stay in the seller's own words, and the
/// customer's turns are shown in the seller's language, so the transcript and
/// the notes read in one language whatever is spoken.
fn engines(translation: Option<&Translation>) -> Result<(Engine, Engine), SessionError> {
    let Some(translation) = translation else {
        return Ok((Engine::Transcribe, Engine::Transcribe));
    };

    let seller = match &translation.outgoing_device_uid {
        None => Engine::Transcribe,
        Some(device_uid) => Engine::Translate {
            config: TranslateConfig {
                target_language: translation.meeting_language.clone(),
                // If the seller switches to the meeting's language, their
                // words must still reach the meeting.
                echo_target_language: true,
            },
            turns: TurnText::Original,
            speech: Some(speech_output(Some(device_uid))?),
        },
    };
    let customer = match translation.incoming {
        Incoming::Off => Engine::Transcribe,
        incoming => Engine::Translate {
            config: TranslateConfig {
                target_language: translation.my_language.clone(),
                // Speech already in the seller's language needs no repeating.
                echo_target_language: false,
            },
            turns: TurnText::Translated,
            speech: match incoming {
                Incoming::Voice => Some(speech_output(None)?),
                _ => None,
            },
        },
    };
    Ok((seller, customer))
}

#[cfg(target_os = "macos")]
fn speech_output(device_uid: Option<&str>) -> Result<Box<dyn SpeechSink>, SessionError> {
    use aura_audio::playback::Playback;

    struct Output(Playback);
    impl SpeechSink for Output {
        fn play(&self, pcm: &[i16]) {
            self.0.write(pcm);
        }
    }
    Playback::open(device_uid, aura_translate::OUTPUT_SAMPLE_RATE, 1.0)
        .map(|playback| Box::new(Output(playback)) as Box<dyn SpeechSink>)
        .map_err(|_| SessionError::AudioOutput)
}

#[cfg(not(target_os = "macos"))]
fn speech_output(_device_uid: Option<&str>) -> Result<Box<dyn SpeechSink>, SessionError> {
    Err(SessionError::AudioOutput)
}

/// Writes up a finished meeting from its transcript and final notes.
pub async fn summarize_meeting(
    credentials: &Credentials,
    language: Option<&str>,
    transcript: &[Turn],
    topics: &[Topic],
) -> Result<MeetingSummary, String> {
    let client = ResponsesClient::new(credentials.openai.clone(), REVIEW_MODEL).map_err(|error| error.to_string())?;
    aura_intel::summarize(&client, language, transcript, topics)
        .await
        .map_err(|error| error.to_string())
}
