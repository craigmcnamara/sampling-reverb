import AVFoundation
import AppKit
import AudioToolbox
import CoreAudioKit

/// Hosts the Sampling Reverb AU in an AVAudioEngine graph: source → AU → output.
/// It always starts with the source Off: live input with speakers feeds back, so listening is a choice.
@MainActor
final class HostModel: ObservableObject {
    enum Source: String, CaseIterable, Identifiable {
        case off = "Off"
        case input = "Input"
        case file = "File"
        var id: String { rawValue }
    }

    @Published var source: Source = .off { didSet { if source != oldValue { restart() } } }
    /// Device UIDs; nil follows the system default. Remembered between launches.
    @Published var inputUID: String? { didSet { deviceChanged(inputUID, oldValue, key: Self.inputKey) } }
    @Published var outputUID: String? { didSet { deviceChanged(outputUID, oldValue, key: Self.outputKey) } }
    @Published var bypass = false { didSet { effect?.auAudioUnit.shouldBypassEffect = bypass } }
    @Published private(set) var viewController: NSViewController?
    @Published private(set) var status = "Loading Audio Unit…"
    @Published private(set) var fileName: String?

    let devices = AudioDevices()

    static let component = AudioComponentDescription(
        componentType: kAudioUnitType_Effect,
        componentSubType: fourCC("Smpv"),
        componentManufacturer: fourCC("Crmc"),
        componentFlags: 0,
        componentFlagsMask: 0)

    private static let inputKey = "inputDeviceUID"
    private static let outputKey = "outputDeviceUID"

    // A new engine for every change of source or device: once an engine has touched its input node
    // it keeps the input open, and switching away from Input must really stop listening.
    private var engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private let sourceMixer = AVAudioMixerNode()
    private var effect: AVAudioUnit?
    private var fileBuffer: AVAudioPCMBuffer?
    private var loaded = false

    init() {
        inputUID = UserDefaults.standard.string(forKey: Self.inputKey)
        outputUID = UserDefaults.standard.string(forKey: Self.outputKey)
    }

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
                self.restart()
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
            if source == .file { restart() } else { source = .file }
        } catch {
            status = "Couldn't open \(url.lastPathComponent): \(error.localizedDescription)"
        }
    }

    // MARK: Graph

    private func deviceChanged(_ uid: String?, _ old: String?, key: String) {
        guard uid != old else { return }
        UserDefaults.standard.set(uid, forKey: key)
        restart()
    }

    /// Tears down the engine and builds a new one for the current source and devices.
    private func restart() {
        guard let unit = effect else { return }
        engine.stop()
        player.stop()
        for node in [player, sourceMixer, unit] where node.engine != nil { engine.detach(node) }
        engine = AVAudioEngine()

        if let output = deviceID(outputUID, input: false), !setDevice(output, on: engine.outputNode) {
            status = "Couldn't use that output device."
        }
        let rate = engine.outputNode.outputFormat(forBus: 0).sampleRate
        guard let stereo = AVAudioFormat(standardFormatWithSampleRate: rate > 0 ? rate : 48000, channels: 2) else { return }
        engine.attach(player)
        engine.attach(sourceMixer)
        engine.attach(unit)
        engine.connect(sourceMixer, to: unit, format: stereo)
        engine.connect(unit, to: engine.mainMixerNode, format: stereo)

        switch source {
        case .off:
            status = "Source is off. Choose Input (use headphones) or File."
        case .input:
            let engine = self.engine
            requestMicrophone { granted in
                guard engine === self.engine, self.source == .input else { return }  // changed meanwhile
                guard granted else {
                    self.status = "Microphone access denied. Enable it in System Settings → Privacy & Security."
                    return
                }
                let input = engine.inputNode
                if let device = self.deviceID(self.inputUID, input: true), !self.setDevice(device, on: input) {
                    self.status = "Couldn't use that input device."
                    return
                }
                engine.connect(input, to: self.sourceMixer, format: input.outputFormat(forBus: 0))
                self.start("Listening to \(self.deviceName(self.inputUID, input: true)). Use headphones to avoid feedback.")
            }
        case .file:
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

    private func deviceID(_ uid: String?, input: Bool) -> AudioDeviceID? {
        guard let uid else { return nil }
        return (input ? devices.inputs : devices.outputs).first { $0.uid == uid }?.id
    }

    private func deviceName(_ uid: String?, input: Bool) -> String {
        let list = input ? devices.inputs : devices.outputs
        let id = deviceID(uid, input: input) ?? AudioDevices.defaultDevice(input: input)
        return list.first { $0.id == id }?.name ?? "the default \(input ? "input" : "output")"
    }

    private func setDevice(_ device: AudioDeviceID, on node: AVAudioIONode) -> Bool {
        guard let unit = node.audioUnit else { return false }
        var id = device
        return AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                    &id, UInt32(MemoryLayout<AudioDeviceID>.size)) == noErr
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
