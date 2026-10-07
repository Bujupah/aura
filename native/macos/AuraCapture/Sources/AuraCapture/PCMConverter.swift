import AVFoundation
import CoreMedia

/// Converts whatever a capture source delivers into the wire format the
/// rest of Aura uses: mono signed 16-bit PCM, at the rate the stream's
/// consumer needs (24 kHz for transcription, 16 kHz for translation).
///
/// One instance per source. It keeps the sample-rate converter's state
/// between buffers so consecutive buffers join without clicks, and rebuilds
/// it when the source format changes (for example, when a headset connects).
final class PCMConverter {
    let outputFormat: AVAudioFormat

    init(sampleRate: Double = 24_000) {
        outputFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true)!
    }

    private var converter: AVAudioConverter?
    private var inputFormat: AVAudioFormat?

    /// Returns nil when the buffer is empty or cannot be converted.
    func convert(_ input: AVAudioPCMBuffer) -> AVAudioPCMBuffer? {
        guard input.frameLength > 0 else { return nil }
        if inputFormat != input.format {
            inputFormat = input.format
            converter = AVAudioConverter(from: input.format, to: outputFormat)
        }
        guard let converter else { return nil }

        let ratio = outputFormat.sampleRate / input.format.sampleRate
        let capacity = AVAudioFrameCount((Double(input.frameLength) * ratio).rounded(.up)) + 32
        guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity)
        else { return nil }

        var supplied = false
        var failure: NSError?
        let status = converter.convert(to: output, error: &failure) { _, inputStatus in
            if supplied {
                // More audio will follow in the next call; keep converter state.
                inputStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
            inputStatus.pointee = .haveData
            return input
        }
        guard status != .error, failure == nil, output.frameLength > 0 else { return nil }
        return output
    }
}

extension AVAudioPCMBuffer {
    /// Copies the samples out of a capture sample buffer.
    static func copying(_ sampleBuffer: CMSampleBuffer) -> AVAudioPCMBuffer? {
        guard let description = sampleBuffer.formatDescription else { return nil }
        let format = AVAudioFormat(cmAudioFormatDescription: description)
        let frames = AVAudioFrameCount(sampleBuffer.numSamples)
        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)
        else { return nil }
        buffer.frameLength = frames
        let status = CMSampleBufferCopyPCMDataIntoAudioBufferList(
            sampleBuffer, at: 0, frameCount: Int32(frames), into: buffer.mutableAudioBufferList)
        return status == noErr ? buffer : nil
    }
}
