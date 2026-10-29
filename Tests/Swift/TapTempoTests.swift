import Testing

@Suite struct TapTempoTests {
    @Test func oneTapGivesNoTempo() {
        var t = TapTempo()
        #expect(t.tap(at: 10) == nil)
    }

    @Test func steadyTapsGiveTheirTempo() {
        var t = TapTempo()
        var bpm: Double?
        for i in 0..<4 { bpm = t.tap(at: 1 + Double(i) * 0.5) }
        #expect(abs((bpm ?? 0) - 120) < 1e-9)
    }

    @Test func tapsAreAveraged() {
        var t = TapTempo()
        _ = t.tap(at: 0)
        _ = t.tap(at: 0.48)
        let bpm = t.tap(at: 1.0) ?? 0  // two intervals averaging 0.5 s
        #expect(abs(bpm - 120) < 1e-9)
    }

    @Test func onlyTheLatestTapsCount() {
        var t = TapTempo(maxTaps: 3)
        for time in [0, 1, 2] as [Double] { _ = t.tap(at: time) }  // 60 BPM
        _ = t.tap(at: 2.5)
        let bpm = t.tap(at: 3.0) ?? 0  // last three taps are 0.5 s apart
        #expect(abs(bpm - 120) < 1e-9)
    }

    @Test func aLongPauseStartsOver() {
        var t = TapTempo()
        _ = t.tap(at: 0)
        _ = t.tap(at: 1)
        #expect(t.tap(at: 10) == nil)
        #expect(abs((t.tap(at: 10.5) ?? 0) - 120) < 1e-9)
    }

    @Test func tempoIsClampedToTheParameterRange() {
        var fast = TapTempo()
        _ = fast.tap(at: 0)
        #expect(fast.tap(at: 0.05) == 240)
        var slow = TapTempo()
        _ = slow.tap(at: 0)
        #expect(slow.tap(at: 1.9) == 40)
    }

    @Test func beatLightIsOnAtTheStartOfEachBeat() {
        #expect(BeatDisplay(tempo: 120, fromHost: true, beatInBar: 0.1).lit)
        #expect(BeatDisplay(tempo: 120, fromHost: true, beatInBar: 0.1).downbeat)
        #expect(!BeatDisplay(tempo: 120, fromHost: true, beatInBar: 0.5).lit)
        let second = BeatDisplay(tempo: 120, fromHost: true, beatInBar: 1.2)
        #expect(second.lit && !second.downbeat)
    }

    @Test func tempoIsShownToATenth() {
        #expect(BeatDisplay(tempo: 119.987, fromHost: false, beatInBar: 0).tempo == 120.0)
    }
}
