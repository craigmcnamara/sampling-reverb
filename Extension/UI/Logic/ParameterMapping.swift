import CoreGraphics
import Foundation

/// Maps a parameter's value range to a 0…1 knob position.
struct ParameterMapping: Equatable {
    enum Curve: Equatable { case linear, exponential, stepped }

    var min: Float
    var max: Float
    var defaultValue: Float
    var curve: Curve

    /// Normalized change per point of vertical drag.
    static let coarseSensitivity: Float = 0.005
    static let fineSensitivity: Float = 0.001

    var isBipolar: Bool { min < 0 && max > 0 }

    func normalized(_ value: Float) -> Float {
        let v = Swift.min(Swift.max(value, min), max)
        switch curve {
        case .linear, .stepped:
            return max > min ? (v - min) / (max - min) : 0
        case .exponential:
            return log(v / min) / log(max / min)
        }
    }

    func value(normalized n: Float) -> Float {
        let t = Swift.min(Swift.max(n, 0), 1)
        switch curve {
        case .linear:
            return min + (max - min) * t
        case .stepped:
            return (min + (max - min) * t).rounded()
        case .exponential:
            return min * pow(max / min, t)
        }
    }

    /// New normalized position after dragging `deltaY` points upwards from `start`.
    func dragged(from start: Float, deltaY: CGFloat, fine: Bool) -> Float {
        let sensitivity = fine ? Self.fineSensitivity : Self.coarseSensitivity
        return Swift.min(Swift.max(start + Float(deltaY) * sensitivity, 0), 1)
    }
}
