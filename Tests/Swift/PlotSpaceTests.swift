import CoreGraphics
import Testing

@Suite struct PlotSpaceTests {
    let plot = PlotSpace(margin: 0.1)

    @Test func centreMapsToOrigin() {
        #expect(plot.toNDC(SIMD2(0.5, 0.5)) == SIMD2(0, 0))
    }

    @Test func cornersRespectTheMargin() {
        let lo = plot.toNDC(SIMD2(0, 0))
        let hi = plot.toNDC(SIMD2(1, 1))
        #expect(abs(lo.x + 0.8) < 1e-6 && abs(lo.y + 0.8) < 1e-6)
        #expect(abs(hi.x - 0.8) < 1e-6 && abs(hi.y - 0.8) < 1e-6)
    }

    @Test func viewPointsInvertTheMapping() {
        let size = CGSize(width: 400, height: 200)
        let p = plot.fromView(CGPoint(x: 200, y: 100), size: size)
        #expect(abs(p.x - 0.5) < 1e-6 && abs(p.y - 0.5) < 1e-6)
        // AppKit views are y-up; 10 % in from the bottom-left edge is plot (0, 0).
        let corner = plot.fromView(CGPoint(x: 40, y: 20), size: size)
        #expect(abs(corner.x) < 1e-6 && abs(corner.y) < 1e-6)
    }

    @Test func viewPointsOutsideThePlotAreClamped() {
        let p = plot.fromView(CGPoint(x: -50, y: 900), size: CGSize(width: 400, height: 200))
        #expect(p == SIMD2(0, 1))
    }
}
