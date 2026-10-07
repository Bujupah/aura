import AVFoundation
import ScreenCaptureKit

enum CaptureSource: Int32 {
    case microphone = 0
    case systemAudio = 1
}

enum CaptureFailure: Error {
    case microphoneDenied
    case screenRecordingDenied
    case noDisplay
    case other(String)
}

/// One ScreenCaptureKit stream with two audio outputs. System audio and the
/// microphone arrive on separate queues and are never mixed.
final class CaptureEngine: NSObject, SCStreamOutput, SCStreamDelegate {
    typealias AudioHandler = (CaptureSource, UnsafePointer<Int16>, Int) -> Void
    typealias StopHandler = (String) -> Void

    private let onAudio: AudioHandler
    private let onStopped: StopHandler
    private let systemQueue = DispatchQueue(label: "aura.capture.system")
    private let microphoneQueue = DispatchQueue(label: "aura.capture.microphone")
    // Each converter is touched only from its own queue.
    private let systemConverter: PCMConverter
    private let microphoneConverter: PCMConverter
    private var stream: SCStream?

    init(
        microphoneRate: Double, systemRate: Double,
        onAudio: @escaping AudioHandler, onStopped: @escaping StopHandler
    ) {
        microphoneConverter = PCMConverter(sampleRate: microphoneRate)
        systemConverter = PCMConverter(sampleRate: systemRate)
        self.onAudio = onAudio
        self.onStopped = onStopped
    }

    func start() async throws {
        guard await AVCaptureDevice.requestAccess(for: .audio) else {
            throw CaptureFailure.microphoneDenied
        }

        let content: SCShareableContent
        do {
            content = try await SCShareableContent.excludingDesktopWindows(
                false, onScreenWindowsOnly: true)
        } catch let error as SCStreamError where error.code == .userDeclined {
            throw CaptureFailure.screenRecordingDenied
        }
        guard let display = content.displays.first else { throw CaptureFailure.noDisplay }

        // Aura never captures itself: its windows are left out of the filter
        // and its own sounds out of the audio.
        let ownPID = ProcessInfo.processInfo.processIdentifier
        let ownApplications = content.applications.filter { $0.processID == ownPID }
        let filter = SCContentFilter(
            display: display, excludingApplications: ownApplications, exceptingWindows: [])

        let configuration = SCStreamConfiguration()
        configuration.capturesAudio = true
        configuration.excludesCurrentProcessAudio = true
        configuration.captureMicrophone = true
        configuration.sampleRate = 48_000
        configuration.channelCount = 2
        // ScreenCaptureKit requires a video configuration even for audio-only
        // use. Ask for the least it will produce; no screen output is
        // attached, so frames are never delivered to Aura.
        configuration.width = 2
        configuration.height = 2
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        configuration.showsCursor = false

        let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: systemQueue)
        try stream.addStreamOutput(self, type: .microphone, sampleHandlerQueue: microphoneQueue)
        do {
            try await stream.startCapture()
        } catch let error as SCStreamError where error.code == .userDeclined {
            throw CaptureFailure.screenRecordingDenied
        }
        self.stream = stream
    }

    /// Returns only after no further audio callback can run.
    func stop() async {
        if let stream {
            self.stream = nil
            try? await stream.stopCapture()
        }
        systemQueue.sync {}
        microphoneQueue.sync {}
    }

    func stream(
        _ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of type: SCStreamOutputType
    ) {
        let source: CaptureSource
        let converter: PCMConverter
        switch type {
        case .audio:
            source = .systemAudio
            converter = systemConverter
        case .microphone:
            source = .microphone
            converter = microphoneConverter
        default:
            return
        }
        guard sampleBuffer.isValid,
            let input = AVAudioPCMBuffer.copying(sampleBuffer),
            let output = converter.convert(input),
            let samples = output.int16ChannelData?[0]
        else { return }
        onAudio(source, samples, Int(output.frameLength))
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        self.stream = nil
        onStopped(error.localizedDescription)
    }
}
