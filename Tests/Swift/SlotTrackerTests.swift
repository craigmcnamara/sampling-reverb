import Testing

@Suite struct SlotTrackerTests {
    func slot(active: Bool = true, generation: UInt32 = 1, triggers: UInt32 = 0) -> SlotSnapshot {
        SlotSnapshot(active: active, generation: generation, triggerCount: triggers,
                     position: SIMD2(0.5, 0.5), life: 1, clarity: 1, level: 0)
    }

    @Test func firstObservationDoesNotReplayHistory() {
        var tracker = SlotTracker(count: 2)
        let events = tracker.update([slot(triggers: 7), slot(active: false)])
        #expect(events.isEmpty)
    }

    @Test func newGenerationIsABirth() {
        var tracker = SlotTracker(count: 1)
        _ = tracker.update([slot(active: false, generation: 0)])
        #expect(tracker.update([slot(generation: 1)]) == [.born(0)])
    }

    @Test func triggerCountIncreaseIsATrigger() {
        var tracker = SlotTracker(count: 1)
        _ = tracker.update([slot(triggers: 2)])
        #expect(tracker.update([slot(triggers: 3)]) == [.triggered(0)])
        #expect(tracker.update([slot(triggers: 3)]).isEmpty)
    }

    @Test func refilledSlotIsABirthNotATrigger() {
        var tracker = SlotTracker(count: 1)
        _ = tracker.update([slot(generation: 1, triggers: 5)])
        #expect(tracker.update([slot(generation: 2, triggers: 5)]) == [.born(0)])
    }

    @Test func deactivationIsADeath() {
        var tracker = SlotTracker(count: 1)
        _ = tracker.update([slot()])
        #expect(tracker.update([slot(active: false)]) == [.died(0)])
    }

    @Test func pitchedAndNoisyColoursDiffer() {
        let pitched = SlotStyle.color(clarity: 1)
        let noisy = SlotStyle.color(clarity: 0)
        #expect(pitched.z > pitched.x)  // cyan
        #expect(noisy.x > noisy.z)      // amber
    }
}
