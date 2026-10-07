import Foundation

// The C surface the Rust core links against. Keep in sync with
// crates/aura-audio/src/capture.rs.

public typealias AuraAudioCallback = @convention(c) (
    UnsafeMutableRawPointer?, Int32, UnsafePointer<Int16>?, Int32
) -> Void
public typealias AuraStateCallback = @convention(c) (
    UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?
) -> Void

private enum StateCode {
    static let running: Int32 = 1
    static let stopped: Int32 = 2
    static let microphoneDenied: Int32 = 10
    static let screenRecordingDenied: Int32 = 11
    static let failed: Int32 = 12
}

/// The context pointer is owned by the caller, which guarantees it stays
/// valid until `aura_capture_stop` returns.
private struct CallerContext: @unchecked Sendable {
    let pointer: UnsafeMutableRawPointer?
}

private let engineLock = NSLock()
nonisolated(unsafe) private var activeEngine: CaptureEngine?

/// Starts capture, delivering each source at its own sample rate. Returns 0
/// if a start was scheduled, -1 if capture is already active. The outcome
/// arrives through `onState`.
@_cdecl("aura_capture_start")
public func auraCaptureStart(
    _ context: UnsafeMutableRawPointer?,
    _ microphoneRate: Int32,
    _ systemRate: Int32,
    _ onAudio: AuraAudioCallback,
    _ onState: AuraStateCallback
) -> Int32 {
    let caller = CallerContext(pointer: context)
    let report: @Sendable (Int32, String) -> Void = { code, detail in
        detail.withCString { onState(caller.pointer, code, $0) }
    }
    let engine = CaptureEngine(
        microphoneRate: Double(microphoneRate), systemRate: Double(systemRate),
        onAudio: { source, samples, count in
            onAudio(caller.pointer, source.rawValue, samples, Int32(count))
        },
        onStopped: { report(StateCode.stopped, $0) })

    engineLock.lock()
    guard activeEngine == nil else {
        engineLock.unlock()
        return -1
    }
    activeEngine = engine
    engineLock.unlock()

    Task.detached {
        do {
            try await engine.start()
            report(StateCode.running, "")
        } catch CaptureFailure.microphoneDenied {
            report(StateCode.microphoneDenied, "")
        } catch CaptureFailure.screenRecordingDenied {
            report(StateCode.screenRecordingDenied, "")
        } catch CaptureFailure.noDisplay {
            report(StateCode.failed, "No display is available to capture audio from.")
        } catch CaptureFailure.other(let message) {
            report(StateCode.failed, message)
        } catch {
            report(StateCode.failed, error.localizedDescription)
        }
    }
    return 0
}

/// Stops capture and blocks until no callback can run any more. Must not be
/// called from the main thread or from inside a callback.
@_cdecl("aura_capture_stop")
public func auraCaptureStop() {
    engineLock.lock()
    let engine = activeEngine
    activeEngine = nil
    engineLock.unlock()
    guard let engine else { return }

    let finished = DispatchSemaphore(value: 0)
    Task.detached {
        await engine.stop()
        finished.signal()
    }
    finished.wait()
}

// MARK: - Playback

/// Opens a player for mono 16-bit PCM at `sampleRate`. `deviceUID` selects an
/// output device; null uses the system default. Returns null on failure.
@_cdecl("aura_playback_open")
public func auraPlaybackOpen(_ deviceUID: UnsafePointer<CChar>?, _ sampleRate: Int32, _ volume: Float)
    -> UnsafeMutableRawPointer?
{
    let uid = deviceUID.map { String(cString: $0) }
    guard let player = try? Player(deviceUID: uid, sampleRate: Double(sampleRate), volume: volume)
    else { return nil }
    return Unmanaged.passRetained(player).toOpaque()
}

@_cdecl("aura_playback_write")
public func auraPlaybackWrite(
    _ handle: UnsafeMutableRawPointer?, _ samples: UnsafePointer<Int16>?, _ count: Int32
) {
    guard let handle, let samples, count > 0 else { return }
    Unmanaged<Player>.fromOpaque(handle).takeUnretainedValue().write(samples, count: Int(count))
}

/// Stops playback and releases the player. The handle is invalid afterwards.
@_cdecl("aura_playback_close")
public func auraPlaybackClose(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    Unmanaged<Player>.fromOpaque(handle).takeRetainedValue().close()
}

/// Finds an audio output device whose name contains `nameFragment`
/// (case-insensitive) and writes its UID into `buffer`. Returns the UID's
/// length, or 0 if there is no such device or the buffer is too small.
@_cdecl("aura_find_output_device")
public func auraFindOutputDevice(
    _ nameFragment: UnsafePointer<CChar>?, _ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int32
) -> Int32 {
    guard let nameFragment, let buffer, capacity > 0,
        let uid = OutputDevices.uid(nameContaining: String(cString: nameFragment))
    else { return 0 }
    let bytes = Array(uid.utf8)
    guard bytes.count < Int(capacity) else { return 0 }
    for (index, byte) in bytes.enumerated() { buffer[index] = CChar(bitPattern: byte) }
    buffer[bytes.count] = 0
    return Int32(bytes.count)
}
