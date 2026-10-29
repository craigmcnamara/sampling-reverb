/// Which factory preset is selected, and whether the current values have drifted from it.
struct PresetSelection: Equatable {
    var selected: Int?

    static let tolerance: Float = 1e-3

    func isModified(current: [UInt64: Float], presets: [[UInt64: Float]], ignoring: Set<UInt64>) -> Bool {
        guard let selected, presets.indices.contains(selected) else { return false }
        for (address, value) in presets[selected] where !ignoring.contains(address) {
            guard let now = current[address] else { continue }
            if abs(now - value) > Swift.max(Self.tolerance, abs(value) * 1e-4) { return true }
        }
        return false
    }
}
