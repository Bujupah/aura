import AVFoundation

/// Checks a loopback device end to end: what is played into it should be
/// heard on its input.
final class LoopbackProbe: NSObject, AVCaptureAudioDataOutputSampleBufferDelegate {
    private let lock = NSLock()
    private var peak: Float = 0

    static func run(deviceUID: String) -> Float {
        guard AVCaptureDevice.authorizationStatus(for: .audio) == .authorized else { return -1 }
        guard let device = AVCaptureDevice(uniqueID: deviceUID),
            let input = try? AVCaptureDeviceInput(device: device)
        else { return -2 }

        let probe = LoopbackProbe()
        let session = AVCaptureSession()
        let output = AVCaptureAudioDataOutput()
        output.setSampleBufferDelegate(probe, queue: DispatchQueue(label: "aura.loopback"))
        guard session.canAddInput(input), session.canAddOutput(output) else { return -3 }
        session.addInput(input)
        session.addOutput(output)
        session.startRunning()
        defer { session.stopRunning() }

        guard let player = try? Player(deviceUID: deviceUID, sampleRate: 24_000, volume: 1) else { return -3 }
        defer { player.close() }
        // Let capture settle, then play 0.6 s of a 440 Hz tone at half scale.
        Thread.sleep(forTimeInterval: 0.3)
        let tone: [Int16] = (0..<14_400).map { Int16(16_000 * sin(2 * Double.pi * 440 * Double($0) / 24_000)) }
        tone.withUnsafeBufferPointer { player.write($0.baseAddress!, count: $0.count) }
        Thread.sleep(forTimeInterval: 1.1)

        probe.lock.lock()
        defer { probe.lock.unlock() }
        return probe.peak
    }

    func captureOutput(
        _ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection
    ) {
        guard let buffer = AVAudioPCMBuffer.copying(sampleBuffer) else { return }
        let frames = Int(buffer.frameLength)
        var loudest: Float = 0
        if let channels = buffer.floatChannelData {
            for channel in 0..<Int(buffer.format.channelCount) {
                for frame in 0..<frames { loudest = max(loudest, abs(channels[channel][frame])) }
            }
        } else if let channels = buffer.int16ChannelData {
            for channel in 0..<Int(buffer.format.channelCount) {
                for frame in 0..<frames {
                    loudest = max(loudest, abs(Float(channels[channel][frame])) / Float(Int16.max))
                }
            }
        }
        lock.lock()
        peak = max(peak, loudest)
        lock.unlock()
    }
}
