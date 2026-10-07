//! Binding to the Swift capture engine (`native/macos/AuraCapture`).

use std::ffi::{c_char, c_void, CStr};

use crate::Source;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureState {
    Running,
    /// The system ended capture, for example when the display changed.
    Stopped(String),
    MicrophoneDenied,
    ScreenRecordingDenied,
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("audio capture is already running")]
    AlreadyRunning,
}

/// Receives audio on the capture queues. Implementations must be quick and
/// must not block: they run on real-time-ish system threads.
pub trait CaptureHandler: Send + Sync + 'static {
    fn audio(&self, source: Source, samples: &[i16]);
    fn state(&self, state: CaptureState);
}

/// A running capture. Dropping it stops capture and waits until no callback
/// can run, so it must not be dropped on the main thread.
pub struct Capture {
    // Double-boxed: the C side needs a thin pointer to a fat `dyn` pointer.
    context: *mut Box<dyn CaptureHandler>,
}

// The context is only dereferenced by the native callbacks, and the handler
// it points to is `Send + Sync`.
unsafe impl Send for Capture {}

extern "C" {
    fn aura_capture_start(
        context: *mut c_void,
        microphone_rate: i32,
        system_rate: i32,
        on_audio: extern "C" fn(*mut c_void, i32, *const i16, i32),
        on_state: extern "C" fn(*mut c_void, i32, *const c_char),
    ) -> i32;
    fn aura_capture_stop();
}

impl Capture {
    /// Requests permissions if needed and starts both sources, each delivered
    /// at its own sample rate. The outcome is reported to `handler.state`.
    pub fn start(
        handler: impl CaptureHandler,
        microphone_rate: u32,
        system_rate: u32,
    ) -> Result<Self, CaptureError> {
        let context: *mut Box<dyn CaptureHandler> = Box::into_raw(Box::new(Box::new(handler)));
        // SAFETY: `context` stays valid until `aura_capture_stop` returns in
        // `drop`, after which the native side makes no further callbacks.
        let status = unsafe {
            aura_capture_start(context.cast(), microphone_rate as i32, system_rate as i32, on_audio, on_state)
        };
        if status != 0 {
            // SAFETY: the native side rejected the start and kept no pointer.
            drop(unsafe { Box::from_raw(context) });
            return Err(CaptureError::AlreadyRunning);
        }
        Ok(Self { context })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: stop blocks until callbacks have drained; only then is the
        // handler freed.
        unsafe {
            aura_capture_stop();
            drop(Box::from_raw(self.context));
        }
    }
}

extern "C" fn on_audio(context: *mut c_void, source: i32, samples: *const i16, count: i32) {
    if context.is_null() || samples.is_null() || count <= 0 {
        return;
    }
    let source = match source {
        0 => Source::Microphone,
        1 => Source::SystemAudio,
        _ => return,
    };
    // SAFETY: the native side passes the context given to `aura_capture_start`
    // and a buffer of `count` samples valid for the duration of this call.
    let (handler, samples) = unsafe {
        (
            &*context.cast::<Box<dyn CaptureHandler>>(),
            std::slice::from_raw_parts(samples, count as usize),
        )
    };
    handler.audio(source, samples);
}

extern "C" fn on_state(context: *mut c_void, code: i32, detail: *const c_char) {
    if context.is_null() {
        return;
    }
    let detail = if detail.is_null() {
        String::new()
    } else {
        // SAFETY: a NUL-terminated string valid for the duration of this call.
        unsafe { CStr::from_ptr(detail) }.to_string_lossy().into_owned()
    };
    let state = match code {
        1 => CaptureState::Running,
        2 => CaptureState::Stopped(detail),
        10 => CaptureState::MicrophoneDenied,
        11 => CaptureState::ScreenRecordingDenied,
        _ => CaptureState::Failed(detail),
    };
    // SAFETY: as in `on_audio`.
    let handler = unsafe { &*context.cast::<Box<dyn CaptureHandler>>() };
    handler.state(state);
}
