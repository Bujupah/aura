//! Audio capture and conditioning.
//!
//! Everything downstream of capture works on mono signed 16-bit PCM. Each
//! stream has the sample rate its consumer needs — [`SAMPLE_RATE`] unless a
//! stream asks otherwise — and the native layer converts to it before audio
//! crosses into Rust.

#[cfg(target_os = "macos")]
pub mod capture;
mod level;
mod pacer;
#[cfg(target_os = "macos")]
pub mod playback;
mod resample;

pub use level::level;
pub use pacer::Pacer;
pub use resample::resample;

pub const SAMPLE_RATE: u32 = 24_000;

/// Which device a buffer came from. Sources are never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Microphone,
    SystemAudio,
}
