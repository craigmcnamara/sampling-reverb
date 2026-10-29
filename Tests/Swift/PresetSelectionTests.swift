import Testing

@Suite struct PresetSelectionTests {
    let presets: [[UInt64: Float]] = [
        [0: 50, 1: 4, 11: 0],
        [0: 80, 1: 12, 11: 0],
    ]
    let freeze: UInt64 = 11

    @Test func nothingSelectedIsNeverModified() {
        let s = PresetSelection(selected: nil)
        #expect(!s.isModified(current: [0: 1, 1: 1], presets: presets, ignoring: [freeze]))
    }

    @Test func matchingValuesAreUnmodified() {
        let s = PresetSelection(selected: 1)
        #expect(!s.isModified(current: [0: 80, 1: 12, 11: 0], presets: presets, ignoring: [freeze]))
    }

    @Test func tinyFloatDifferencesAreTolerated() {
        let s = PresetSelection(selected: 1)
        #expect(!s.isModified(current: [0: 80.0001, 1: 12, 11: 0], presets: presets, ignoring: [freeze]))
    }

    @Test func aChangedKnobIsModified() {
        let s = PresetSelection(selected: 0)
        #expect(s.isModified(current: [0: 50, 1: 6, 11: 0], presets: presets, ignoring: [freeze]))
    }

    @Test func ignoredParametersDoNotCount() {
        let s = PresetSelection(selected: 0)
        #expect(!s.isModified(current: [0: 50, 1: 4, 11: 1], presets: presets, ignoring: [freeze]))
    }

    @Test func outOfRangeSelectionIsSafe() {
        let s = PresetSelection(selected: 9)
        #expect(!s.isModified(current: [0: 50], presets: presets, ignoring: []))
    }
}
