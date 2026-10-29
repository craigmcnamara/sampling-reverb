import simd

/// Vertex for point sprites and trace lines. Layout must match `PointVertex` in Shaders.metal.
struct PointVertex {
    var position: SIMD2<Float>  // NDC
    var size: Float             // pixels
    var pad: Float = 0
    var color: SIMD4<Float>     // HDR, additive
}

struct Particle {
    var position: SIMD2<Float>  // plot space, 0…1
    var velocity: SIMD2<Float>
    var life: Float
    var maxLife: Float
    var color: SIMD3<Float>
    var size: Float
}

/// CPU particle simulation for the playback bursts. Positions live in plot space.
final class ParticleSystem {
    let capacity: Int
    private(set) var particles: [Particle] = []
    private var rng: SplitMix64

    static let drag: Float = 2.2       // velocity e-folding per second
    static let lift: Float = 0.03      // upward drift, plot units / s²

    init(capacity: Int = 4096, seed: UInt64 = 0x5EED) {
        self.capacity = capacity
        self.rng = SplitMix64(seed: seed)
        particles.reserveCapacity(capacity)
    }

    /// A radial explosion, used when a sample starts playing.
    func burst(at origin: SIMD2<Float>, color: SIMD3<Float>, count: Int, speed: Float = 0.35) {
        for _ in 0..<count {
            let angle = rng.float() * 2 * .pi
            let s = speed * (0.3 + 0.7 * rng.float())
            spawn(Particle(position: origin,
                           velocity: SIMD2(cos(angle), sin(angle)) * s,
                           life: 0, maxLife: 0.6 + 0.9 * rng.float(),
                           color: color, size: 2 + 3 * rng.float()))
        }
    }

    /// A gentle trickle, used while a sample keeps playing.
    func emit(at origin: SIMD2<Float>, color: SIMD3<Float>, count: Int) {
        burst(at: origin, color: color, count: count, speed: 0.08)
    }

    func update(dt: Float) {
        let damping = exp(-Self.drag * dt)
        for i in particles.indices {
            particles[i].velocity *= damping
            particles[i].velocity.y += Self.lift * dt
            particles[i].position += particles[i].velocity * dt
            particles[i].life += dt
        }
        particles.removeAll { $0.life >= $0.maxLife }
    }

    func reset() { particles.removeAll(keepingCapacity: true) }

    func appendVertices(to out: inout [PointVertex], plot: PlotSpace, pixelScale: Float, gain: Float = 1) {
        for p in particles {
            let fade = 1 - p.life / p.maxLife
            let intensity = fade * fade * 0.9 * gain
            out.append(PointVertex(position: plot.toNDC(p.position),
                                   size: p.size * pixelScale * (0.6 + 0.4 * fade),
                                   color: SIMD4(p.color * intensity, 1)))
        }
    }

    private func spawn(_ p: Particle) {
        if particles.count >= capacity { particles.removeFirst(particles.count - capacity + 1) }
        particles.append(p)
    }
}

/// Small deterministic PRNG so tests (and visuals) are reproducible.
struct SplitMix64 {
    private var state: UInt64
    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    mutating func float() -> Float { Float(next() >> 40) / Float(1 << 24) }
}
