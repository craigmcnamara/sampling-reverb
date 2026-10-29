import AVFoundation
import AppKit
import AudioToolbox
import CoreAudioKit

/// Hosts the Sampling Reverb AU in an AVAudioEngine graph: source → AU → output.
@MainActor
final class HostModel: ObservableObject {
    enum Source: String, CaseIterable, Identifiable {
        case input = "Audio Input"
        case file = "Audio File"
        var id: String { rawValue }
    }

    @Published var source: Source = .input { didSet { if source != oldValue { reconnectSource() } } }
    @Published var bypass = false { didSet { effect?.auAudioUnit.shouldBypassEffect = bypass } }
    @Published private(set) var viewController: NSViewController?
    @Published private(set) var status = "Loading Audio Unit…"
    @Published private(set) var fileName: String?

    static let component = AudioComponentDescription(
        componentType: kAudioUnitType_Effect,
        componentSubType: fourCC("Smpv"),
        componentManufacturer: fourCC("Crmc"),
        componentFlags: 0,
        componentFlagsMask: 0)

    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private let sourceMixer = AVAudioMixerNode()
    private var effect: AVAudioUnit?
    private var fileBuffer: AVAudioPCMBuffer?
    private var loaded = false

    func load() {
        guard !loaded else { return }
        loaded = true
        attemptLoad(1)
    }

    private static let loadTimeout: TimeInterval = 8
    private static let maxLoadAttempts = 3

    @MainActor private final class Attempt { var finished = false }

    /// Instantiation never completes if macOS is still registering a freshly installed extension,
    /// so each attempt has a deadline and is retried rather than waiting forever.
    private func attemptLoad(_ number: Int) {
        let attempt = Attempt()
        AVAudioUnit.instantiate(with: Self.component, options: .loadOutOfProcess) { unit, error in
            Task { @MainActor in
                guard !attempt.finished else { return }  // this attempt already timed out
                attempt.finished = true
                guard let unit else {
                    self.status = "Couldn't load the Audio Unit: \(error?.localizedDescription ?? "not found")"
                    return
                }
                self.effect = unit
                self.buildGraph(unit)
                self.viewController = await unit.auAudioUnit.requestViewController()
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.loadTimeout) {
            guard !attempt.finished else { return }
            attempt.finished = true
            if number < Self.maxLoadAttempts {
                self.status = "Waiting for macOS to register the Audio Unit… (retry \(number))"
                self.attemptLoad(number + 1)
            } else {
                self.status = "The Audio Unit didn't load. Quit and reopen the app."
            }
        }
    }

    func openFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.audio]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let file = try AVAudioFile(forReading: url)
            let frames = AVAudioFrameCount(min(file.length, AVAudioFramePosition(file.processingFormat.sampleRate * 600)))
            guard let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: frames) else { return }
            try file.read(into: buffer, frameCount: frames)
            fileBuffer = buffer
            fileName = url.lastPathComponent
            if source == .file { reconnectSource() } else { source = .file }
        } catch {
            status = "Couldn't open \(url.lastPathComponent): \(error.localizedDescription)"
        }
    }

    // MARK: Graph

    private func buildGraph(_ unit: AVAudioUnit) {
        let rate = engine.outputNode.outputFormat(forBus: 0).sampleRate
        guard let stereo = AVAudioFormat(standardFormatWithSampleRate: rate > 0 ? rate : 48000, channels: 2) else { return }
        engine.attach(player)
        engine.attach(sourceMixer)
        engine.attach(unit)
        engine.connect(sourceMixer, to: unit, format: stereo)
        engine.connect(unit, to: engine.mainMixerNode, format: stereo)
        reconnectSource()
    }

    private func reconnectSource() {
        guard effect != nil else { return }
        engine.stop()
        player.stop()
        engine.disconnectNodeOutput(player)
        if source == .input {
            requestMicrophone { granted in
                guard granted else {
                    self.status = "Microphone access denied. Enable it in System Settings → Privacy & Security."
                    return
                }
                let input = self.engine.inputNode
                self.engine.connect(input, to: self.sourceMixer, format: input.outputFormat(forBus: 0))
                self.start("Listening to audio input. Use headphones to avoid feedback.")
            }
        } else {
            engine.inputNode.reset()
            engine.disconnectNodeOutput(engine.inputNode)
            guard let buffer = fileBuffer else {
                status = "Choose an audio file with Open File…"
                return
            }
            engine.connect(player, to: sourceMixer, format: buffer.format)
            start("Looping \(fileName ?? "file")")
            player.scheduleBuffer(buffer, at: nil, options: .loops)
            player.play()
        }
    }

    private func start(_ message: String) {
        do {
            engine.prepare()
            try engine.start()
            status = message
        } catch {
            status = "Audio engine failed to start: \(error.localizedDescription)"
        }
    }

    private func requestMicrophone(_ done: @escaping @MainActor (Bool) -> Void) {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            done(true)
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { granted in Task { @MainActor in done(granted) } }
        default:
            done(false)
        }
    }
}

func fourCC(_ s: String) -> FourCharCode {
    s.utf8.prefix(4).reduce(0) { ($0 << 8) | FourCharCode($1) }
}
