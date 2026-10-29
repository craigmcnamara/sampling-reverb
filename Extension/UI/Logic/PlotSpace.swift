import CoreGraphics
import simd

/// The scope's 0…1 plot square, inset by a margin, and its mapping to Metal NDC and view points.
struct PlotSpace: Equatable {
    var margin: Float = 0.06

    func toNDC(_ p: SIMD2<Float>) -> SIMD2<Float> {
        let span = 1 - 2 * margin
        return (SIMD2(repeating: margin) + p * span) * 2 - 1
    }

    /// `point` is in y-up view coordinates (AppKit's default).
    func fromView(_ point: CGPoint, size: CGSize) -> SIMD2<Float> {
        guard size.width > 0, size.height > 0 else { return SIMD2(0.5, 0.5) }
        let uv = SIMD2(Float(point.x / size.width), Float(point.y / size.height))
        let p = (uv - SIMD2(repeating: margin)) / (1 - 2 * margin)
        return simd_clamp(p, SIMD2(repeating: 0), SIMD2(repeating: 1))
    }
}
