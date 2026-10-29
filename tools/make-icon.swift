// Renders the app icon: a phosphor oscilloscope floating in deep space, with sample points and one
// sample blooming into the reverb.
//
//   swift tools/make-icon.swift App/Assets.xcassets/AppIcon.appiconset
import AppKit
import CoreGraphics

let size: CGFloat = 1024
let inset: CGFloat = 100           // macOS icon grid: 824 pt artwork inside a 1024 canvas
let corner: CGFloat = 185

struct Rand {
    var s: UInt64
    mutating func next() -> CGFloat {
        s = s &* 6364136223846793005 &+ 1442695040888963407
        return CGFloat(s >> 40) / CGFloat(1 << 24)
    }
}

func color(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r, green: g, blue: b, alpha: a)
}

func render() -> CGImage {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let art = CGRect(x: inset, y: inset, width: size - 2 * inset, height: size - 2 * inset)
    let shape = CGPath(roundedRect: art, cornerWidth: corner, cornerHeight: corner, transform: nil)
    let c = CGPoint(x: size / 2, y: size / 2)
    var rng = Rand(s: 7)

    // Drop shadow under the tile.
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28, color: color(0, 0, 0, 0.45))
    ctx.addPath(shape)
    ctx.setFillColor(color(0.02, 0.03, 0.05))
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(shape)
    ctx.clip()

    // Deep space.
    let space = CGGradient(colorsSpace: cs, colors: [color(0.05, 0.11, 0.15), color(0.015, 0.025, 0.045)] as CFArray,
                           locations: [0, 1])!
    ctx.drawRadialGradient(space, startCenter: CGPoint(x: c.x - 40, y: c.y + 60), startRadius: 0,
                           endCenter: c, endRadius: 560, options: [.drawsAfterEndLocation])

    // Nebula wisps.
    for (x, y, r, col) in [(330.0, 650.0, 260.0, color(0.15, 0.55, 0.6, 0.20)),
                           (690.0, 380.0, 300.0, color(0.35, 0.2, 0.6, 0.16)),
                           (600.0, 700.0, 180.0, color(0.1, 0.4, 0.5, 0.12))] {
        let g = CGGradient(colorsSpace: cs, colors: [col, col.copy(alpha: 0)!] as CFArray, locations: [0, 1])!
        ctx.drawRadialGradient(g, startCenter: CGPoint(x: x, y: y), startRadius: 0,
                               endCenter: CGPoint(x: x, y: y), endRadius: r, options: [])
    }

    // Stars.
    for _ in 0..<220 {
        let p = CGPoint(x: inset + rng.next() * art.width, y: inset + rng.next() * art.height)
        let r = 0.6 + rng.next() * 1.8
        ctx.setFillColor(color(0.8, 0.95, 1, 0.25 + rng.next() * 0.6))
        ctx.fillEllipse(in: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
    }

    // Oscilloscope graticule.
    let plot = art.insetBy(dx: 90, dy: 90)
    ctx.setLineWidth(1.5)
    ctx.setStrokeColor(color(0.3, 0.85, 0.75, 0.10))
    for i in 0...8 {
        let x = plot.minX + plot.width * CGFloat(i) / 8
        let y = plot.minY + plot.height * CGFloat(i) / 8
        ctx.move(to: CGPoint(x: x, y: plot.minY)); ctx.addLine(to: CGPoint(x: x, y: plot.maxY))
        ctx.move(to: CGPoint(x: plot.minX, y: y)); ctx.addLine(to: CGPoint(x: plot.maxX, y: y))
    }
    ctx.strokePath()
    ctx.setStrokeColor(color(0.3, 0.85, 0.75, 0.22))
    ctx.setLineWidth(2)
    ctx.move(to: CGPoint(x: c.x, y: plot.minY)); ctx.addLine(to: CGPoint(x: c.x, y: plot.maxY))
    ctx.move(to: CGPoint(x: plot.minX, y: c.y)); ctx.addLine(to: CGPoint(x: plot.maxX, y: c.y))
    ctx.strokePath()

    // Glowing 3:2 Lissajous figure, followed by two fainter, smaller echoes (the reverb decaying).
    func lissajous(scale: CGFloat, rotation: CGFloat) -> CGPath {
        let path = CGMutablePath()
        let steps = 900
        for i in 0...steps {
            let t = CGFloat(i) / CGFloat(steps) * 2 * .pi
            let x = 215 * scale * sin(3 * t + .pi / 2), y = 190 * scale * sin(2 * t)
            let p = CGPoint(x: c.x + x * cos(rotation) - y * sin(rotation),
                            y: c.y + 20 + x * sin(rotation) + y * cos(rotation))
            i == 0 ? path.move(to: p) : path.addLine(to: p)
        }
        path.closeSubpath()
        return path
    }
    for (scale, rotation, strength) in [(0.62, -0.30, 0.25), (0.81, -0.15, 0.5), (1.0, 0.0, 1.0)] as [(CGFloat, CGFloat, CGFloat)] {
        let path = lissajous(scale: scale, rotation: rotation)
        for (width, alpha, blur) in [(16.0, 0.10, 40.0), (7.0, 0.30, 18.0), (2.8, 0.95, 6.0)] as [(CGFloat, CGFloat, CGFloat)] {
            ctx.saveGState()
            ctx.setShadow(offset: .zero, blur: blur, color: color(0.3, 1, 0.85, 0.9 * strength))
            ctx.addPath(path)
            ctx.setLineWidth(width * (0.6 + 0.4 * strength))
            ctx.setLineJoin(.round)
            ctx.setStrokeColor(color(0.45, 1, 0.88, alpha * strength))
            ctx.strokePath()
            ctx.restoreGState()
        }
    }

    func glow(_ p: CGPoint, _ r: CGFloat, _ col: CGColor, core: CGColor = color(1, 1, 1, 1)) {
        let g = CGGradient(colorsSpace: cs, colors: [col, col.copy(alpha: 0)!] as CFArray, locations: [0, 1])!
        ctx.drawRadialGradient(g, startCenter: p, startRadius: 0, endCenter: p, endRadius: r * 5, options: [])
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: r * 3, color: col)
        ctx.setFillColor(core)
        ctx.fillEllipse(in: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
        ctx.restoreGState()
    }

    // Sample points in the timbre space.
    let cyan = color(0.35, 1, 0.85, 0.55)
    for (x, y, r) in [(255.0, 690.0, 9.0), (705.0, 735.0, 7.0), (770.0, 520.0, 10.0),
                      (330.0, 330.0, 8.0), (560.0, 250.0, 6.0), (440.0, 790.0, 6.0)] {
        glow(CGPoint(x: x, y: y), r, cyan)
    }

    // One sample playing: amber burst with expanding reverb rings and particle streaks.
    let burst = CGPoint(x: 720, y: 300)
    let amber = color(1, 0.62, 0.22, 0.9)
    for (radius, alpha) in [(135.0, 0.16), (95.0, 0.28), (58.0, 0.45)] {
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 12, color: amber)
        ctx.setStrokeColor(color(1, 0.7, 0.3, alpha))
        ctx.setLineWidth(3)
        ctx.strokeEllipse(in: CGRect(x: burst.x - radius, y: burst.y - radius, width: 2 * radius, height: 2 * radius))
        ctx.restoreGState()
    }
    for _ in 0..<40 {
        let a = rng.next() * 2 * .pi
        let d0 = 22 + rng.next() * 25
        let d1 = d0 + 15 + rng.next() * 55
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 8, color: amber)
        ctx.setStrokeColor(color(1, 0.75 + rng.next() * 0.2, 0.4, 0.35 + rng.next() * 0.5))
        ctx.setLineWidth(1.5 + rng.next() * 2)
        ctx.setLineCap(.round)
        ctx.move(to: CGPoint(x: burst.x + cos(a) * d0, y: burst.y + sin(a) * d0))
        ctx.addLine(to: CGPoint(x: burst.x + cos(a) * d1, y: burst.y + sin(a) * d1))
        ctx.strokePath()
        ctx.restoreGState()
    }
    glow(burst, 18, color(1, 0.62, 0.22, 1), core: color(1, 0.95, 0.85))

    // Vignette and a soft top highlight on the glass.
    let vignette = CGGradient(colorsSpace: cs, colors: [color(0, 0, 0, 0), color(0, 0, 0, 0.55)] as CFArray,
                              locations: [0.55, 1])!
    ctx.drawRadialGradient(vignette, startCenter: c, startRadius: 0, endCenter: c, endRadius: 600, options: [])
    let sheen = CGGradient(colorsSpace: cs, colors: [color(1, 1, 1, 0.10), color(1, 1, 1, 0)] as CFArray,
                           locations: [0, 1])!
    ctx.drawLinearGradient(sheen, start: CGPoint(x: c.x, y: art.maxY), end: CGPoint(x: c.x, y: art.maxY - 260), options: [])
    ctx.restoreGState()

    // Fine rim.
    ctx.addPath(shape)
    ctx.setStrokeColor(color(0.5, 0.9, 0.85, 0.25))
    ctx.setLineWidth(2)
    ctx.strokePath()
    return ctx.makeImage()!
}

func writePNG(_ image: CGImage, pixels: Int, to url: URL) {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

let outDir = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
let image = render()
var entries: [String] = []
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = "icon_\(points)x\(points)\(scale == 2 ? "@2x" : "").png"
        writePNG(image, pixels: points * scale, to: outDir.appendingPathComponent(name))
        entries.append("""
            { "idiom" : "mac", "size" : "\(points)x\(points)", "scale" : "\(scale)x", "filename" : "\(name)" }
        """)
    }
}
let contents = "{\n  \"images\" : [\n\(entries.joined(separator: ",\n"))\n  ],\n  \"info\" : { \"author\" : \"xcode\", \"version\" : 1 }\n}\n"
try contents.write(to: outDir.appendingPathComponent("Contents.json"), atomically: true, encoding: .utf8)
print("wrote \(entries.count) icon sizes to \(outDir.path)")
