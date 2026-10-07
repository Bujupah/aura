//! Plays PCM through an output device, via the Swift layer.

use std::ffi::{c_char, c_void, CString};

extern "C" {
    fn aura_playback_open(device_uid: *const c_char, sample_rate: i32, volume: f32) -> *mut c_void;
    fn aura_playback_write(handle: *mut c_void, samples: *const i16, count: i32);
    fn aura_playback_close(handle: *mut c_void);
    fn aura_find_output_device(name_fragment: *const c_char, buffer: *mut c_char, capacity: i32) -> i32;
    fn aura_default_output_device(buffer: *mut c_char, capacity: i32) -> i32;
    fn aura_loopback_test(device_uid: *const c_char) -> f32;
}

#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    #[error("the audio output could not be opened")]
    Unavailable,
}

/// An open player for mono 16-bit PCM. Dropping it stops playback.
pub struct Playback {
    handle: *mut c_void,
}

// The Swift player only ever schedules buffers on an `AVAudioPlayerNode`,
// which may be done from any thread.
unsafe impl Send for Playback {}
unsafe impl Sync for Playback {}

impl Playback {
    /// `device_uid` of `None` plays on the system's default output.
    pub fn open(device_uid: Option<&str>, sample_rate: u32, volume: f32) -> Result<Self, PlaybackError> {
        let uid = device_uid
            .map(CString::new)
            .transpose()
            .map_err(|_| PlaybackError::Unavailable)?;
        let pointer = uid.as_ref().map_or(std::ptr::null(), |uid| uid.as_ptr());
        // SAFETY: `pointer` is null or a NUL-terminated string that outlives
        // the call.
        let handle = unsafe { aura_playback_open(pointer, sample_rate as i32, volume) };
        if handle.is_null() {
            return Err(PlaybackError::Unavailable);
        }
        Ok(Self { handle })
    }

    /// Queues samples after everything already queued.
    pub fn write(&self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        // SAFETY: the handle is live until `drop`; the samples are copied
        // before the call returns.
        unsafe { aura_playback_write(self.handle, samples.as_ptr(), samples.len() as i32) };
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        // SAFETY: the handle came from `aura_playback_open` and is closed once.
        unsafe { aura_playback_close(self.handle) };
    }
}

/// UID of the first output device whose name contains `name_fragment`,
/// ignoring case.
pub fn find_output_device(name_fragment: &str) -> Option<String> {
    let fragment = CString::new(name_fragment).ok()?;
    let mut buffer = [0 as c_char; 256];
    // SAFETY: the buffer's real capacity is passed; the callee NUL-terminates.
    let length = unsafe { aura_find_output_device(fragment.as_ptr(), buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer[..length as usize].iter().map(|&c| c as u8).collect();
    String::from_utf8(bytes).ok()
}

/// UID of the device the system plays sound on by default.
pub fn default_output_device() -> Option<String> {
    let mut buffer = [0 as c_char; 256];
    // SAFETY: the buffer's real capacity is passed; the callee NUL-terminates.
    let length = unsafe { aura_default_output_device(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer[..length as usize].iter().map(|&c| c as u8).collect();
    String::from_utf8(bytes).ok()
}

/// What playing a test tone into a loopback device and listening on its
/// input found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Loopback {
    /// The tone came back; the value is its peak level, 0 to 1.
    Heard(f32),
    /// Nothing came back.
    Silent,
    /// Aura may not use the microphone, so it cannot listen to the device.
    NoPermission,
    /// The device is missing or has no input side.
    NoDevice,
    Failed,
}

/// Quieter than this is noise, not the test tone.
const HEARD_THRESHOLD: f32 = 0.05;

/// Plays a short tone into the device and checks it arrives on the device's
/// input. Blocks for about a second and a half.
pub fn loopback_test(device_uid: &str) -> Loopback {
    let Ok(uid) = CString::new(device_uid) else {
        return Loopback::NoDevice;
    };
    // SAFETY: a NUL-terminated string that outlives the call.
    interpret(unsafe { aura_loopback_test(uid.as_ptr()) })
}

fn interpret(result: f32) -> Loopback {
    match result {
        peak if peak >= HEARD_THRESHOLD => Loopback::Heard(peak),
        peak if peak >= 0.0 => Loopback::Silent,
        code if code > -1.5 => Loopback::NoPermission,
        code if code > -2.5 => Loopback::NoDevice,
        _ => Loopback::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_results_map_to_outcomes() {
        assert_eq!(interpret(0.48), Loopback::Heard(0.48));
        assert_eq!(interpret(0.01), Loopback::Silent);
        assert_eq!(interpret(0.0), Loopback::Silent);
        assert_eq!(interpret(-1.0), Loopback::NoPermission);
        assert_eq!(interpret(-2.0), Loopback::NoDevice);
        assert_eq!(interpret(-3.0), Loopback::Failed);
    }
}
