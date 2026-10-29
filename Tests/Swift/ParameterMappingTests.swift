import Testing

@Suite struct ParameterMappingTests {
    let mix = ParameterMapping(min: 0, max: 100, defaultValue: 50, curve: .linear)
    let decay = ParameterMapping(min: 0.3, max: 30, defaultValue: 4, curve: .exponential)
    let interval = ParameterMapping(min: 0, max: 5, defaultValue: 4, curve: .stepped)
    let filter = ParameterMapping(min: -100, max: 100, defaultValue: 0, curve: .linear)

    @Test func linearEndpointsAndMidpoint() {
        #expect(mix.normalized(0) == 0)
        #expect(mix.normalized(100) == 1)
        #expect(mix.normalized(25) == 0.25)
        #expect(mix.value(normalized: 0.75) == 75)
    }

    @Test func exponentialMidpointIsTheGeometricMean() {
        #expect(abs(decay.normalized(3) - 0.5) < 1e-5)
        #expect(abs(decay.value(normalized: 0.5) - 3) < 1e-4)
        #expect(abs(decay.value(normalized: 0) - 0.3) < 1e-6)
        #expect(abs(decay.value(normalized: 1) - 30) < 1e-4)
    }

    @Test func steppedSnapsToIntegers() {
        #expect(interval.value(normalized: 0.49) == 2)
        #expect(interval.value(normalized: 0.51) == 3)
        #expect(interval.normalized(4) == 0.8)
    }

    @Test func clampsOutOfRange() {
        #expect(mix.normalized(-10) == 0)
        #expect(mix.normalized(500) == 1)
        #expect(mix.value(normalized: 2) == 100)
        #expect(decay.value(normalized: -1) == 0.3)
    }

    @Test(arguments: [0.3, 1, 4, 12.5, 30] as [Float])
    func exponentialRoundTrips(_ value: Float) {
        #expect(abs(decay.value(normalized: decay.normalized(value)) - value) < value * 1e-4)
    }

    @Test func bipolarDetection() {
        #expect(filter.isBipolar)
        #expect(!mix.isBipolar)
        #expect(filter.normalized(0) == 0.5)
    }

    @Test func dragUpIncreasesAndFineDragIsSlower() {
        #expect(abs(mix.dragged(from: 0.2, deltaY: 100, fine: false) - 0.7) < 1e-5)
        #expect(abs(mix.dragged(from: 0.2, deltaY: 100, fine: true) - 0.3) < 1e-5)
        #expect(abs(mix.dragged(from: 0.2, deltaY: -100, fine: false) - 0.0) < 1e-5)
        #expect(mix.dragged(from: 0.9, deltaY: 1000, fine: false) == 1)
    }
}
