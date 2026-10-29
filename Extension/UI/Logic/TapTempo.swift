import Foundation

/// Tap tempo: averages the intervals between the last few taps. A pause longer than `timeout`
/// starts a new count.
struct TapTempo {
    var maxTaps = 5
    var timeout: TimeInterval = 2
    var range: ClosedRange<Double> = 40...240

    private(set) var taps: [TimeInterval] = []

    /// Records a tap at `time` (seconds) and returns the new tempo once there are two taps.
    mutating func tap(at time: TimeInterval) -> Double? {
        if let last = taps.last, time - last > timeout || time <= last { taps.removeAll() }
        taps.append(time)
        if taps.count > maxTaps { taps.removeFirst(taps.count - maxTaps) }
        guard taps.count >= 2, let first = taps.first, let last = taps.last else { return nil }
        let interval = (last - first) / Double(taps.count - 1)
        return min(max(60 / interval, range.lowerBound), range.upperBound)
    }
}

/// What the clock readout shows, derived from the plugin's tempo and position in the bar.
struct BeatDisplay: Equatable {
    var tempo: Double
    var fromHost: Bool
    /// The beat light is on for the first quarter of each beat.
    var lit: Bool
    /// The lit beat is the first of the bar.
    var downbeat: Bool

    init(tempo: Double, fromHost: Bool, beatInBar: Double) {
        self.tempo = (tempo * 10).rounded() / 10
        self.fromHost = fromHost
        let beat = max(beatInBar, 0)
        lit = beat - beat.rounded(.down) < 0.25
        downbeat = beat < 1
    }
}
