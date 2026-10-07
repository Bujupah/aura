//! One speaker's path: pacer → realtime session → turns.
//!
//! A stream runs on one of two engines. *Transcribe* sends the audio to
//! GPT-Live and turns its transcript into speaker turns. *Translate* sends it
//! to Gemini Live Translate, which interprets continuously: the translated
//! speech can be played somewhere, and either the original or the translated
//! text becomes the turns.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use aura_audio::Pacer;
use aura_core::transcript::{Speaker, Turn, TurnAssembler};
use aura_live::{ApiCredential, LiveConnection, LiveReceiver, LiveSender, ServerEvent, SessionConfig};
use aura_translate::{
    is_language, TranslateConfig, TranslateConnection, TranslateEvent, TranslateReceiver,
    TranslateSender,
};
use tokio::sync::{mpsc, watch};

use crate::{prompt, FailureReason, ListeningState, SessionError, SessionEvent};

const MODEL: &str = "gpt-live-1";
const TICK: Duration = Duration::from_millis(100);
/// The most audio held if the network stalls, in seconds; older audio is
/// dropped.
const BUFFER_SECONDS: usize = 5;
const QUIET_CHECK: Duration = Duration::from_millis(250);
/// Transcript text trails the audio it describes (about 0.4 s observed,
/// sometimes more). The silence check waits this much longer than the turn
/// gap so a late fragment is not mistaken for a pause.
const TRANSCRIPT_LAG_ALLOWANCE_MS: u64 = 1_200;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(15);
const PROGRESS_REPORT: Duration = Duration::from_secs(10);

pub type SharedPacer = Arc<Mutex<Pacer>>;

/// Where translated speech is played.
pub trait SpeechSink: Send + Sync {
    fn play(&self, pcm: &[i16]);
}

/// Which text of a translated stream becomes the speaker's turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnText {
    /// What the speaker said, in their own words.
    Original,
    /// The translation — or the original when the speaker was already using
    /// the target language and nothing was translated.
    Translated,
}

pub enum Engine {
    Transcribe,
    Translate {
        config: TranslateConfig,
        turns: TurnText,
        /// Receives the translated speech, if it should be heard.
        speech: Option<Box<dyn SpeechSink>>,
    },
}

impl Engine {
    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::Transcribe => aura_audio::SAMPLE_RATE,
            Self::Translate { .. } => aura_translate::INPUT_SAMPLE_RATE,
        }
    }
}

pub struct Credentials {
    pub openai: ApiCredential,
    /// Needed only by streams that translate.
    pub gemini: Option<ApiCredential>,
}

pub struct SpeakerStream {
    speaker: Speaker,
    engine: Engine,
    pacer: SharedPacer,
    /// Numbers new turns after those of the meeting being continued.
    assembler: TurnAssembler,
}

pub enum Connected {
    Live(LiveConnection),
    Translate(TranslateConnection),
}

impl SpeakerStream {
    /// `earlier` holds the turns of a meeting being continued, if any.
    pub fn new(speaker: Speaker, engine: Engine, earlier: &[Turn]) -> Self {
        let rate = engine.sample_rate() as usize;
        let tick = rate / 1000 * TICK.as_millis() as usize;
        Self {
            speaker,
            engine,
            pacer: Arc::new(Mutex::new(Pacer::new(tick, rate * BUFFER_SECONDS))),
            assembler: TurnAssembler::resuming(speaker, earlier),
        }
    }

    pub fn pacer(&self) -> SharedPacer {
        self.pacer.clone()
    }

    pub fn sample_rate(&self) -> u32 {
        self.engine.sample_rate()
    }

    pub async fn connect(&self, credentials: &Credentials) -> Result<Connected, SessionError> {
        match &self.engine {
            Engine::Transcribe => {
                let config = SessionConfig::new(MODEL, prompt::listener(self.speaker));
                Ok(Connected::Live(aura_live::connect(&credentials.openai, config).await?))
            }
            Engine::Translate { config, .. } => {
                let credential = credentials.gemini.as_ref().ok_or(SessionError::NoTranslationCredential)?;
                Ok(Connected::Translate(aura_translate::connect(credential, config).await?))
            }
        }
    }

    /// Runs until stopped or the connection fails. Returns the seconds the
    /// provider billed, when it reports them.
    pub async fn run(
        self,
        connection: Connected,
        clock: Instant,
        events: mpsc::UnboundedSender<SessionEvent>,
        final_turns: mpsc::UnboundedSender<Turn>,
        stopped: watch::Receiver<bool>,
    ) -> Option<f64> {
        let speaker = self.speaker;
        let turns = TurnSink {
            assembler: self.assembler,
            events: events.clone(),
            final_turns,
        };

        let outcome = match (connection, self.engine) {
            (Connected::Live(connection), _) => {
                tracing::info!(event = "stream_started", ?speaker, engine = "transcribe", session = %connection.session_id);
                let (sender, receiver) = connection.split();
                let (first_audio, first_audio_at) = watch::channel(None::<Instant>);
                let sending = tokio::spawn(send_to_live(sender, self.pacer, first_audio, stopped.clone()));
                let outcome = tokio::select! {
                    outcome = receive_live(receiver, turns, clock, first_audio_at) => outcome,
                    // Once asked to stop, the provider gets a bounded time to finalize.
                    _ = close_deadline(stopped) => Ok(None),
                };
                // The sender owns the socket's write half; without the read
                // half it has nothing left to do.
                sending.abort();
                outcome
            }
            (Connected::Translate(connection), Engine::Translate { config, turns: text, speech }) => {
                tracing::info!(event = "stream_started", ?speaker, engine = "translate", target = %config.target_language);
                let (sender, receiver) = connection.split();
                let sending = tokio::spawn(send_to_translate(sender, self.pacer, stopped.clone()));
                let outcome = tokio::select! {
                    outcome = receive_translation(receiver, turns, clock, &config.target_language, text, speech) => outcome,
                    _ = close_deadline(stopped) => Ok(None),
                };
                sending.abort();
                outcome
            }
            (Connected::Translate(_), Engine::Transcribe) => unreachable!("a stream connects with its own engine"),
        };

        match outcome {
            Ok(seconds) => {
                tracing::info!(event = "stream_closed", ?speaker, seconds);
                seconds
            }
            Err(message) => {
                tracing::error!(event = "stream_failed", ?speaker, %message);
                let _ = events.send(SessionEvent::State {
                    state: ListeningState::Failed {
                        reason: FailureReason::Service,
                        message,
                    },
                });
                None
            }
        }
    }
}

/// Builds turns from text fragments and sends them on.
struct TurnSink {
    assembler: TurnAssembler,
    events: mpsc::UnboundedSender<SessionEvent>,
    final_turns: mpsc::UnboundedSender<Turn>,
}

impl TurnSink {
    fn push(&mut self, text: &str, start_ms: u64, end_ms: u64) {
        for turn in self.assembler.push(text, start_ms, end_ms) {
            self.emit(turn);
        }
    }

    /// Adds what the interpreter said to the speaker's turn. The turn's own
    /// text is unchanged, so this is shown but not sent on for note-taking.
    fn push_translation(&mut self, text: &str) {
        if let Some(turn) = self.assembler.push_translation(text) {
            let _ = self.events.send(SessionEvent::Turn { turn });
        }
    }

    fn close_if_quiet(&mut self, clock: Instant) {
        let heard_until_ms = (clock.elapsed().as_millis() as u64).saturating_sub(TRANSCRIPT_LAG_ALLOWANCE_MS);
        if let Some(turn) = self.assembler.close_if_quiet(heard_until_ms) {
            self.emit(turn);
        }
    }

    fn finish(&mut self) {
        if let Some(turn) = self.assembler.finish() {
            self.emit(turn);
        }
    }

    fn emit(&self, turn: Turn) {
        if turn.is_final {
            let _ = self.final_turns.send(turn.clone());
        }
        let _ = self.events.send(SessionEvent::Turn { turn });
    }
}

async fn close_deadline(mut stopped: watch::Receiver<bool>) {
    let _ = stopped.changed().await;
    tokio::time::sleep(CLOSE_TIMEOUT).await;
}

fn drain(pacer: &SharedPacer, samples: &mut Vec<i16>, bytes: &mut Vec<u8>) {
    pacer.lock().unwrap_or_else(PoisonError::into_inner).drain_tick(samples);
    bytes.clear();
    bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
}

async fn send_to_live(
    mut sender: LiveSender,
    pacer: SharedPacer,
    first_audio: watch::Sender<Option<Instant>>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    let (mut samples, mut bytes) = (Vec::new(), Vec::new());
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                drain(&pacer, &mut samples, &mut bytes);
                // The session's timeline starts with its first audio.
                first_audio.send_if_modified(|at| at.is_none() && { *at = Some(Instant::now()); true });
                if sender.append_audio(&bytes).await.is_err() {
                    return;
                }
            }
            _ = stopped.changed() => {
                let _ = sender.close().await;
                return;
            }
        }
    }
}

async fn send_to_translate(mut sender: TranslateSender, pacer: SharedPacer, mut stopped: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    let (mut samples, mut bytes) = (Vec::new(), Vec::new());
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                drain(&pacer, &mut samples, &mut bytes);
                if sender.append_audio(&bytes).await.is_err() {
                    return;
                }
            }
            _ = stopped.changed() => {
                let _ = sender.close().await;
                return;
            }
        }
    }
}

async fn receive_live(
    mut receiver: LiveReceiver,
    mut turns: TurnSink,
    clock: Instant,
    first_audio_at: watch::Receiver<Option<Instant>>,
) -> Result<Option<f64>, String> {
    let speaker = turns.assembler_speaker();
    let mut quiet_check = tokio::time::interval(QUIET_CHECK);
    // Session timestamps count from the first audio sent; this converts them
    // to the meeting clock shared by both streams.
    let offset_ms = move || {
        first_audio_at
            .borrow()
            .map_or(0, |at| at.saturating_duration_since(clock).as_millis() as u64)
    };

    loop {
        tokio::select! {
            event = receiver.next_event() => match event {
                Some(Ok(ServerEvent::InputTranscript(fragment))) => {
                    let offset = offset_ms();
                    turns.push(&fragment.delta, offset + fragment.start_ms, offset + fragment.end_ms);
                }
                Some(Ok(ServerEvent::DelegationCreated { offset_ms: at, .. })) => {
                    // The model thinks this moment needs an answer. Nothing
                    // acts on it yet; it becomes a reasoning trigger later.
                    tracing::info!(event = "attention_requested", ?speaker, at_ms = offset_ms() + at);
                }
                Some(Ok(ServerEvent::SessionClosed { usage })) => {
                    turns.finish();
                    return Ok(usage.get("seconds").and_then(|seconds| seconds.as_f64()));
                }
                Some(Ok(ServerEvent::Error(error))) => {
                    tracing::warn!(event = "live_error", ?speaker, %error);
                }
                // The model's own speech and its transcript are deliberately
                // dropped: an observing session has nothing to say.
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    turns.finish();
                    return Err(error.to_string());
                }
                None => {
                    turns.finish();
                    return Ok(None);
                }
            },
            _ = quiet_check.tick() => turns.close_if_quiet(clock),
        }
    }
}

async fn receive_translation(
    mut receiver: TranslateReceiver,
    mut turns: TurnSink,
    clock: Instant,
    target_language: &str,
    text: TurnText,
    speech: Option<Box<dyn SpeechSink>>,
) -> Result<Option<f64>, String> {
    let speaker = turns.assembler_speaker();
    let mut quiet_check = tokio::time::interval(QUIET_CHECK);
    let mut progress_report = tokio::time::interval(PROGRESS_REPORT);
    // Whether the speaker is currently using the target language already, in
    // which case there is no translation and the original text is the turn.
    let mut speaking_target = false;
    let mut samples: Vec<i16> = Vec::new();
    // Counts only, never content: enough to see in a log which stage of
    // hear → translate → speak has gone quiet.
    let (mut heard_chars, mut said_chars, mut speech_samples) = (0usize, 0usize, 0usize);

    loop {
        tokio::select! {
            events = receiver.next_events() => match events {
                Some(Ok(events)) => {
                    // The service does not timestamp text; arrival time on
                    // the meeting clock is the best available.
                    let now_ms = clock.elapsed().as_millis() as u64;
                    for event in events {
                        match event {
                            TranslateEvent::Heard { text: heard, language } => {
                                heard_chars += heard.chars().count();
                                if let Some(language) = language {
                                    speaking_target = is_language(&language, target_language);
                                }
                                if text == TurnText::Original || speaking_target {
                                    turns.push(&heard, now_ms, now_ms);
                                }
                            }
                            TranslateEvent::Said { text: said } => {
                                said_chars += said.chars().count();
                                match text {
                                    TurnText::Translated if !speaking_target => turns.push(&said, now_ms, now_ms),
                                    TurnText::Translated => {}
                                    // The speaker's own words are the turn;
                                    // show what was said on their behalf.
                                    TurnText::Original => turns.push_translation(&said),
                                }
                            }
                            TranslateEvent::Audio(pcm) => {
                                speech_samples += pcm.len() / 2;
                                if let Some(speech) = &speech {
                                    samples.clear();
                                    samples.extend(pcm.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)));
                                    speech.play(&samples);
                                }
                            }
                        }
                    }
                }
                Some(Err(error)) => {
                    turns.finish();
                    return Err(error.to_string());
                }
                None => {
                    turns.finish();
                    return Ok(None);
                }
            },
            _ = quiet_check.tick() => turns.close_if_quiet(clock),
            _ = progress_report.tick() => {
                tracing::info!(
                    event = "translation_progress",
                    ?speaker,
                    heard_chars,
                    said_chars,
                    speech_seconds = speech_samples as f32 / aura_translate::OUTPUT_SAMPLE_RATE as f32,
                    speech_played = speech.is_some()
                );
            }
        }
    }
}

impl TurnSink {
    fn assembler_speaker(&self) -> Speaker {
        self.assembler.speaker()
    }
}
