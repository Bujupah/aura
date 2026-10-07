import AVFoundation
import XCTest

@testable import AuraCapture

final class PCMConverterTests: XCTestCase {
    /// A 440 Hz tone in the shape ScreenCaptureKit delivers system audio:
    /// 48 kHz, stereo, 32-bit float, non-interleaved.
    private func tone(frames: AVAudioFrameCount, sampleRate: Double = 48_000, channels: UInt32 = 2)
        -> AVAudioPCMBuffer
    {
        let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: channels,
            interleaved: false)!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        for channel in 0..<Int(channels) {
            let samples = buffer.floatChannelData![channel]
            for frame in 0..<Int(frames) {
                samples[frame] = 0.5 * sinf(2 * .pi * 440 * Float(frame) / Float(sampleRate))
            }
        }
        return buffer
    }

    func testProducesMono16BitAt24kHz() throws {
        let output = try XCTUnwrap(PCMConverter().convert(tone(frames: 4800)))
        XCTAssertEqual(output.format.sampleRate, 24_000)
        XCTAssertEqual(output.format.channelCount, 1)
        XCTAssertEqual(output.format.commonFormat, .pcmFormatInt16)
    }

    func testKeepsDurationAndLoudnessAcrossConsecutiveBuffers() throws {
        let converter = PCMConverter()
        var samples: [Int16] = []
        // One second of audio in ten buffers, as a live stream would arrive.
        for _ in 0..<10 {
            let output = try XCTUnwrap(converter.convert(tone(frames: 4800)))
            let data = try XCTUnwrap(output.int16ChannelData?[0])
            samples.append(contentsOf: UnsafeBufferPointer(start: data, count: Int(output.frameLength)))
        }
        // The resampler may hold back a few frames of latency, never more.
        XCTAssertEqual(Double(samples.count), 24_000, accuracy: 240)
        let peak = samples.map { abs(Int($0)) }.max() ?? 0
        XCTAssertEqual(Double(peak), 0.5 * 32_767, accuracy: 0.05 * 32_767)
    }

    func testAdaptsWhenTheSourceFormatChanges() throws {
        let converter = PCMConverter()
        _ = try XCTUnwrap(converter.convert(tone(frames: 4800)))
        // A Bluetooth headset connects: the microphone becomes 16 kHz mono.
        let output = try XCTUnwrap(
            converter.convert(tone(frames: 1600, sampleRate: 16_000, channels: 1)))
        XCTAssertEqual(Double(output.frameLength), 2400, accuracy: 120)
    }

    func testConvertsToTheRateItsConsumerNeeds() throws {
        // Translation takes 16 kHz. One second in, one second out, give or
        // take the resampler's start-up latency.
        let converter = PCMConverter(sampleRate: 16_000)
        var frames = 0
        for _ in 0..<10 {
            let output = try XCTUnwrap(converter.convert(tone(frames: 4800)))
            XCTAssertEqual(output.format.sampleRate, 16_000)
            frames += Int(output.frameLength)
        }
        XCTAssertEqual(Double(frames), 16_000, accuracy: 400)
    }

    func testEmptyInputYieldsNothing() {
        XCTAssertNil(PCMConverter().convert(tone(frames: 0)))
    }
}
