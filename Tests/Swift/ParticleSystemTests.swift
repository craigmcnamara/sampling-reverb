import simd
import Testing

@Suite struct ParticleSystemTests {
    let cyan = SIMD3<Float>(0.3, 1, 0.9)

    @Test func burstSpawnsParticlesAtTheOrigin() {
        let ps = ParticleSystem(capacity: 100, seed: 1)
        ps.burst(at: SIMD2(0.25, 0.75), color: cyan, count: 40)
        #expect(ps.particles.count == 40)
        #expect(ps.particles.allSatisfy { $0.position == SIMD2(0.25, 0.75) })
        #expect(ps.particles.allSatisfy { simd_length($0.velocity) > 0 })
    }

    @Test func capacityIsNeverExceeded() {
        let ps = ParticleSystem(capacity: 50, seed: 1)
        for _ in 0..<5 { ps.burst(at: SIMD2(0.5, 0.5), color: cyan, count: 30) }
        #expect(ps.particles.count == 50)
    }

    @Test func particlesMoveOutwardAndSlowDown() {
        let ps = ParticleSystem(capacity: 10, seed: 3)
        ps.burst(at: SIMD2(0.5, 0.5), color: cyan, count: 1)
        let v0 = simd_length(ps.particles[0].velocity)
        ps.update(dt: 0.1)
        let p = ps.particles[0]
        #expect(simd_distance(p.position, SIMD2(0.5, 0.5)) > 0)
        #expect(simd_length(p.velocity) < v0)
    }

    @Test func particlesExpire() {
        let ps = ParticleSystem(capacity: 100, seed: 1)
        ps.burst(at: SIMD2(0.5, 0.5), color: cyan, count: 20)
        for _ in 0..<200 { ps.update(dt: 1.0 / 60) }  // longer than the maximum particle life
        #expect(ps.particles.isEmpty)
    }

    @Test func vertexIntensityFadesWithAge() {
        let ps = ParticleSystem(capacity: 10, seed: 1)
        ps.burst(at: SIMD2(0.5, 0.5), color: cyan, count: 1)
        let plot = PlotSpace()
        var out: [PointVertex] = []
        ps.appendVertices(to: &out, plot: plot, pixelScale: 2)
        let fresh = out[0].color.y
        ps.update(dt: 0.3)
        out.removeAll()
        ps.appendVertices(to: &out, plot: plot, pixelScale: 2)
        #expect(out[0].color.y < fresh)
    }

    @Test func pointVertexMatchesTheMetalLayout() {
        #expect(MemoryLayout<PointVertex>.stride == 32)
        #expect(MemoryLayout<PointVertex>.offset(of: \.color) == 16)
    }
}
