import simd

/// Swift-side copy of one pool slot, decoupled from the C `SRSlotVisual` type.
struct SlotSnapshot: Equatable {
    var active: Bool
    var generation: UInt32
    var triggerCount: UInt32
    var position: SIMD2<Float>
    var life: Float
    var clarity: Float
    var level: Float
}

/// Turns successive pool snapshots into animation events.
struct SlotTracker {
    enum Event: Equatable {
        case born(Int)
        case triggered(Int)
        case died(Int)
    }

    private var previous: [SlotSnapshot?]

    init(count: Int) { previous = Array(repeating: nil, count: count) }

    mutating func update(_ slots: [SlotSnapshot]) -> [Event] {
        var events: [Event] = []
        for (i, slot) in slots.enumerated() where i < previous.count {
            defer { previous[i] = slot }
            guard let old = previous[i] else { continue }
            if slot.active && (!old.active || slot.generation != old.generation) {
                events.append(.born(i))
            } else if slot.active && slot.triggerCount != old.triggerCount {
                events.append(.triggered(i))
            } else if !slot.active && old.active {
                events.append(.died(i))
            }
        }
        return events
    }
}

enum SlotStyle {
    static let pitched = SIMD3<Float>(0.35, 1.0, 0.85)
    static let noisy = SIMD3<Float>(1.0, 0.62, 0.22)

    static func color(clarity: Float) -> SIMD3<Float> {
        let t = simd_smoothstep(0.3, 0.8, clarity)
        return simd_mix(noisy, pitched, SIMD3(repeating: t))
    }
}
