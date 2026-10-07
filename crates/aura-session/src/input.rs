//! Feeds the two pacers, from the capture engine or from fixture files.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use aura_audio::{level, resample, Source, SAMPLE_RATE};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::stream::SharedPacer;
use crate::{AudioInput, FailureReason, ListeningState, SessionError, SessionEvent};

/// Loudest level seen per source since the last read, as `f32` bits.
#[derive(Default)]
pub struct Levels {
    seller: AtomicU32,
    customer: AtomicU32,
}

impl Levels {
    fn record(&self, source: Source, value: f32) {
        let slot = match source {
            Source::Microphone => &self.seller,
            Source::SystemAudio => &self.customer,
        };
        // Levels are non-negative, so their bit patterns order like the floats.
        slot.fetch_max(value.to_bits(), Ordering::Relaxed);
    }

    /// Returns `(seller, customer)` and resets both.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.seller.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.customer.swap(0, Ordering::Relaxed)),
        )
    }
}

struct Router {
    seller: SharedPacer,
    customer: SharedPacer,
    levels: Arc<Levels>,
}

impl Router {
    fn audio(&self, source: Source, samples: &[i16]) {
        self.levels.record(source, level(samples));
        let pacer = match source {
            Source::Microphone => &self.seller,
            Source::SystemAudio => &self.customer,
        };
        pacer.lock().unwrap_or_else(PoisonError::into_inner).push(samples);
    }
}

pub enum Input {
    #[cfg(target_os = "macos")]
    Capture(aura_audio::capture::Capture),
    Fixtures(Vec<JoinHandle<()>>),
}

impl Input {
    /// `seller_rate` and `customer_rate` are the sample rates the two
    /// streams' engines expect.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        audio: AudioInput,
        seller: SharedPacer,
        customer: SharedPacer,
        seller_rate: u32,
        customer_rate: u32,
        levels: Arc<Levels>,
        events: mpsc::UnboundedSender<SessionEvent>,
        stopped: watch::Receiver<bool>,
    ) -> Result<Self, SessionError> {
        let router = Arc::new(Router {
            seller,
            customer,
            levels,
        });
        match audio {
            AudioInput::Fixtures { seller, customer } => {
                let mut players = Vec::with_capacity(2);
                for (source, path, rate) in [
                    (Source::Microphone, seller, seller_rate),
                    (Source::SystemAudio, customer, customer_rate),
                ] {
                    // Fixtures are recorded at the default rate.
                    let samples = resample(&read_pcm(&path).await?, SAMPLE_RATE, rate);
                    players.push(tokio::spawn(play(source, samples, rate, router.clone(), stopped.clone())));
                }
                Ok(Self::Fixtures(players))
            }
            #[cfg(target_os = "macos")]
            AudioInput::Capture => {
                let handler = CaptureBridge { router, events };
                aura_audio::capture::Capture::start(handler, seller_rate, customer_rate)
                    .map(Self::Capture)
                    .map_err(|_| SessionError::CaptureBusy)
            }
            #[cfg(not(target_os = "macos"))]
            AudioInput::Capture => {
                let _ = events;
                Err(SessionError::CaptureBusy)
            }
        }
    }

    pub async fn stop(self) {
        match self {
            #[cfg(target_os = "macos")]
            // Stopping capture blocks until its callbacks drain.
            Self::Capture(capture) => {
                let _ = tokio::task::spawn_blocking(move || drop(capture)).await;
            }
            Self::Fixtures(players) => {
                for player in players {
                    let _ = player.await;
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct CaptureBridge {
    router: Arc<Router>,
    events: mpsc::UnboundedSender<SessionEvent>,
}

#[cfg(target_os = "macos")]
impl aura_audio::capture::CaptureHandler for CaptureBridge {
    fn audio(&self, source: Source, samples: &[i16]) {
        self.router.audio(source, samples);
    }

    fn state(&self, state: aura_audio::capture::CaptureState) {
        use aura_audio::capture::CaptureState;
        let (reason, message) = match state {
            CaptureState::Running => {
                tracing::info!(event = "capture_running");
                return;
            }
            CaptureState::MicrophoneDenied => (
                FailureReason::MicrophoneDenied,
                "Aura needs microphone access. Allow it in System Settings → Privacy & Security → Microphone.".to_owned(),
            ),
            CaptureState::ScreenRecordingDenied => (
                FailureReason::ScreenRecordingDenied,
                "Aura needs Screen & System Audio Recording access to hear the meeting. Allow it in System Settings → Privacy & Security, then start again.".to_owned(),
            ),
            CaptureState::Stopped(detail) => (FailureReason::Capture, format!("Audio capture stopped: {detail}")),
            CaptureState::Failed(detail) => (FailureReason::Capture, format!("Audio capture failed: {detail}")),
        };
        tracing::error!(event = "capture_failed", ?reason, %message);
        let _ = self.events.send(SessionEvent::State {
            state: ListeningState::Failed { reason, message },
        });
    }
}

async fn read_pcm(path: &Path) -> Result<Vec<i16>, SessionError> {
    let bytes = tokio::fs::read(path).await.map_err(|source| SessionError::Fixture {
        path: path.to_owned(),
        source,
    })?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect())
}

/// Delivers a clip in 20 ms buffers at real-time pace, like a capture device,
/// then goes quiet.
async fn play(
    source: Source,
    samples: Vec<i16>,
    rate: u32,
    router: Arc<Router>,
    mut stopped: watch::Receiver<bool>,
) {
    const BUFFER: Duration = Duration::from_millis(20);
    let buffer_samples = rate as usize / 50;
    let mut ticker = tokio::time::interval(BUFFER);
    for buffer in samples.chunks(buffer_samples) {
        tokio::select! {
            _ = ticker.tick() => router.audio(source, buffer),
            _ = stopped.changed() => return,
        }
    }
    let _ = stopped.changed().await;
}
