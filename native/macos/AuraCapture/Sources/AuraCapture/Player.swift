import AVFoundation
import CoreAudio

/// Plays a stream of mono 16-bit PCM on one output device. Used for
/// translated speech: to the seller's own output, or into a virtual
/// microphone that a meeting app listens to.
final class Player {
    enum Failure: Error {
        case unknownDevice
        case deviceRefused(OSStatus)
    }

    private let engine = AVAudioEngine()
    private let node = AVAudioPlayerNode()
    private let format: AVAudioFormat

    init(deviceUID: String?, sampleRate: Double, volume: Float) throws {
        format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: 1, interleaved: false)!

        if let deviceUID {
            guard var device = OutputDevices.id(forUID: deviceUID) else { throw Failure.unknownDevice }
            guard let unit = engine.outputNode.audioUnit else { throw Failure.unknownDevice }
            let status = AudioUnitSetProperty(
                unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &device,
                UInt32(MemoryLayout<AudioDeviceID>.size))
            guard status == noErr else { throw Failure.deviceRefused(status) }
        }

        engine.attach(node)
        // The mixer resamples to whatever the device runs at.
        engine.connect(node, to: engine.mainMixerNode, format: format)
        node.volume = volume
        try engine.start()
        node.play()
    }

    /// Queues samples to play after everything already queued.
    func write(_ samples: UnsafePointer<Int16>, count: Int) {
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count)),
            let channel = buffer.floatChannelData?[0]
        else { return }
        buffer.frameLength = AVAudioFrameCount(count)
        for index in 0..<count {
            channel[index] = Float(samples[index]) / Float(Int16.max)
        }
        node.scheduleBuffer(buffer)
    }

    func close() {
        node.stop()
        engine.stop()
    }
}

enum OutputDevices {
    /// UID of the first output device whose name contains `fragment`.
    static func uid(nameContaining fragment: String) -> String? {
        all().first { $0.name.localizedCaseInsensitiveContains(fragment) }?.uid
    }

    static func id(forUID uid: String) -> AudioDeviceID? {
        all().first { $0.uid == uid }?.id
    }

    private static func all() -> [(id: AudioDeviceID, name: String, uid: String)] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        let system = AudioObjectID(kAudioObjectSystemObject)
        guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr else { return [] }
        var ids = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &ids) == noErr else { return [] }

        return ids.compactMap { id in
            guard hasOutput(id), let name = string(id, kAudioObjectPropertyName),
                let uid = string(id, kAudioDevicePropertyDeviceUID)
            else { return nil }
            return (id, name, uid)
        }
    }

    private static func hasOutput(_ id: AudioDeviceID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreams, mScope: kAudioObjectPropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        return AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr && size > 0
    }

    private static func string(_ id: AudioDeviceID, _ selector: AudioObjectPropertySelector) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        let status = withUnsafeMutablePointer(to: &value) {
            AudioObjectGetPropertyData(id, &address, 0, nil, &size, $0)
        }
        guard status == noErr, let value else { return nil }
        return value.takeRetainedValue() as String
    }
}
