import AudioToolbox
import Combine
import Foundation
import QuartzCore

/// The tempo readout and beat light. Separate from PluginModel because it changes several times a
/// beat, and only the clock strip should redraw for that.
final class ClockModel: ObservableObject {
    @Published var display = BeatDisplay(tempo: 120, fromHost: false, beatInBar: 0)
}

/// Bridges the AU parameter tree to SwiftUI. Main thread only.
final class PluginModel: ObservableObject {
    let audioUnit: SRAudioUnit
    @Published private(set) var values: [SRParam: Float] = [:]
    @Published var poolCount = 0
    @Published var frozen = false
    /// The scope is showing in its own full-screen window (and the inline one is paused).
    @Published var scopeFullScreen = false
    let clock = ClockModel()
    @Published private(set) var presetSelection = PresetSelection(selected: nil)
    let presetNames: [String]

    private var token: AUParameterObserverToken?
    private var presetObservation: NSKeyValueObservation?
    private var mappings: [SRParam: ParameterMapping] = [:]
    private let presetValues: [[UInt64: Float]]
    private var tapTempo = TapTempo()
    /// Performance controls that presets leave alone (see presetLeavesAlone in ParamInfo.hpp).
    private static let presetIgnored: Set<UInt64> = Set([SRParam.freeze, .quantize, .grid, .tempo].map(\.rawValue))

    init(audioUnit: SRAudioUnit) {
        self.audioUnit = audioUnit
        let presets = audioUnit.factoryPresets ?? []
        presetNames = presets.map(\.name)
        presetValues = presets.indices.map { i in
            Dictionary(uniqueKeysWithValues: audioUnit.valuesForFactoryPreset(at: i).map { ($0.key.uint64Value, $0.value.floatValue) })
        }
        presetObservation = audioUnit.observe(\.currentPreset, options: [.initial, .new]) { [weak self] unit, _ in
            let number = unit.currentPreset?.number
            DispatchQueue.main.async {
                self?.presetSelection = PresetSelection(selected: number.flatMap { $0 >= 0 ? $0 : nil })
            }
        }
        guard let tree = audioUnit.parameterTree else { return }
        for param in tree.allParameters {
            guard let id = SRParam(rawValue: param.address) else { continue }
            values[id] = param.value
            mappings[id] = Self.mapping(for: param)
        }
        token = tree.token(byAddingParameterObserver: { [weak self] address, value in
            DispatchQueue.main.async {
                guard let self, let id = SRParam(rawValue: address) else { return }
                self.values[id] = value
            }
        })
    }

    deinit {
        if let token { audioUnit.parameterTree?.removeParameterObserver(token) }
    }

    func value(_ id: SRParam) -> Float { values[id] ?? 0 }

    func mapping(_ id: SRParam) -> ParameterMapping {
        mappings[id] ?? ParameterMapping(min: 0, max: 1, defaultValue: 0, curve: .linear)
    }

    func normalized(_ id: SRParam) -> Float { mapping(id).normalized(value(id)) }

    func set(_ id: SRParam, _ value: Float) {
        values[id] = value
        audioUnit.parameterTree?.parameter(withAddress: id.rawValue)?.setValue(value, originator: token)
    }

    func setNormalized(_ id: SRParam, _ n: Float) { set(id, mapping(id).value(normalized: n)) }

    func reset(_ id: SRParam) { set(id, mapping(id).defaultValue) }

    var isPresetModified: Bool {
        let current = Dictionary(uniqueKeysWithValues: values.map { ($0.key.rawValue, $0.value) })
        return presetSelection.isModified(current: current, presets: presetValues, ignoring: Self.presetIgnored)
    }

    func applyPreset(_ index: Int) {
        guard let preset = audioUnit.factoryPresets?[safe: index] else { return }
        audioUnit.currentPreset = preset
        presetSelection = PresetSelection(selected: index)
        // The tree notifies our observer asynchronously; update now so the UI doesn't flash "modified".
        for (address, value) in presetValues[index] where !Self.presetIgnored.contains(address) {
            if let id = SRParam(rawValue: address) { values[id] = value }
        }
    }

    /// Tap tempo. Only meaningful without a host tempo, which always wins.
    func tap() {
        if let bpm = tapTempo.tap(at: CACurrentMediaTime()) { set(.tempo, Float(bpm)) }
    }

    /// Reads the tempo and beat position from the audio unit; called once per scope frame.
    func refreshClock() {
        var fromHost: ObjCBool = false
        var beat = 0.0
        let tempo = audioUnit.currentTempo(fromHost: &fromHost, beatInBar: &beat)
        let display = BeatDisplay(tempo: tempo, fromHost: fromHost.boolValue, beatInBar: beat)
        if clock.display != display { clock.display = display }
    }

    /// The names of an indexed parameter's values (e.g. Grid's subdivisions).
    func valueNames(_ id: SRParam) -> [String] {
        audioUnit.parameterTree?.parameter(withAddress: id.rawValue)?.valueStrings ?? []
    }

    func display(_ id: SRParam) -> String {
        guard let param = audioUnit.parameterTree?.parameter(withAddress: id.rawValue) else { return "" }
        var v = value(id)
        return param.string(fromValue: &v)
    }

    private static func mapping(for param: AUParameter) -> ParameterMapping {
        let curve: ParameterMapping.Curve
        if param.flags.contains(.flag_DisplayExponential) {
            curve = .exponential
        } else if param.unit == .indexed || param.unit == .boolean || param.address == SRParam.density.rawValue {
            curve = .stepped
        } else {
            curve = .linear
        }
        let def = param.value  // the tree is created with defaults
        return ParameterMapping(min: param.minValue, max: param.maxValue, defaultValue: def, curve: curve)
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
