import MetalKit
import QuartzCore

/// Must match CompositeUniforms in Shaders.metal.
struct CompositeUniforms {
    var focus: SIMD2<Float>
    var focusAmount: Float
    var bloomStrength: Float
    var resolution: SIMD2<Float>
    var margin: Float
    var exposure: Float
}

/// Phosphor-oscilloscope renderer: HDR persistence buffer + particles + bloom + graticule.
final class ScopeRenderer: NSObject, MTKViewDelegate {
    private weak var model: PluginModel?
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private let fadePipeline, pointPipeline, linePipeline, downsamplePipeline, blurPipeline, compositePipeline: MTLRenderPipelineState

    private var accum: MTLTexture?
    private var bloomA: MTLTexture?
    private var bloomB: MTLTexture?
    private var needsClear = true

    private let plot = PlotSpace()
    private let particles = ParticleSystem()
    private var tracker = SlotTracker(count: Int(SR_MAX_SLOTS))
    private var slots = [SRSlotVisual](repeating: SRSlotVisual(), count: Int(SR_MAX_SLOTS))
    private var scopeL = [Float](repeating: 0, count: Int(SR_SCOPE_POINTS))
    private var scopeR = [Float](repeating: 0, count: Int(SR_SCOPE_POINTS))
    private var emitCarry = [Float](repeating: 0, count: Int(SR_MAX_SLOTS))
    private var traceGain: Float = 4
    private var lastTime = CACurrentMediaTime()
    private var vertices: [PointVertex] = []
    private var trace: [PointVertex] = []

    private static let accumFormat = MTLPixelFormat.rgba16Float

    init?(view: MTKView, model: PluginModel) {
        guard let device = view.device ?? MTLCreateSystemDefaultDevice(),
              let queue = device.makeCommandQueue(),
              let library = try? device.makeDefaultLibrary(bundle: Bundle(for: ScopeRenderer.self))
        else { return nil }
        self.device = device
        self.queue = queue
        self.model = model
        view.device = device
        view.colorPixelFormat = .bgra8Unorm
        view.clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)

        func pipeline(_ vertex: String, _ fragment: String, _ format: MTLPixelFormat,
                      blend: ((MTLRenderPipelineColorAttachmentDescriptor) -> Void)? = nil) -> MTLRenderPipelineState? {
            let d = MTLRenderPipelineDescriptor()
            d.vertexFunction = library.makeFunction(name: vertex)
            d.fragmentFunction = library.makeFunction(name: fragment)
            d.colorAttachments[0].pixelFormat = format
            if let blend {
                d.colorAttachments[0].isBlendingEnabled = true
                blend(d.colorAttachments[0])
            }
            return try? device.makeRenderPipelineState(descriptor: d)
        }
        let additive: (MTLRenderPipelineColorAttachmentDescriptor) -> Void = {
            $0.sourceRGBBlendFactor = .one
            $0.destinationRGBBlendFactor = .one
            $0.sourceAlphaBlendFactor = .one
            $0.destinationAlphaBlendFactor = .one
        }
        let multiply: (MTLRenderPipelineColorAttachmentDescriptor) -> Void = {
            $0.sourceRGBBlendFactor = .zero
            $0.destinationRGBBlendFactor = .sourceColor
            $0.sourceAlphaBlendFactor = .zero
            $0.destinationAlphaBlendFactor = .sourceAlpha
        }
        guard let fade = pipeline("fullscreen_vertex", "fade_fragment", Self.accumFormat, blend: multiply),
              let point = pipeline("point_vertex", "point_fragment", Self.accumFormat, blend: additive),
              let line = pipeline("line_vertex", "line_fragment", Self.accumFormat, blend: additive),
              let down = pipeline("fullscreen_vertex", "downsample_fragment", Self.accumFormat),
              let blur = pipeline("fullscreen_vertex", "blur_fragment", Self.accumFormat),
              let comp = pipeline("fullscreen_vertex", "composite_fragment", view.colorPixelFormat)
        else { return nil }
        fadePipeline = fade
        pointPipeline = point
        linePipeline = line
        downsamplePipeline = down
        blurPipeline = blur
        compositePipeline = comp
        super.init()
    }

    // MARK: MTKViewDelegate

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        makeTextures(size)
    }

    func draw(in view: MTKView) {
        let now = CACurrentMediaTime()
        let dt = Float(min(max(now - lastTime, 0), 0.1))
        lastTime = now

        if accum == nil { makeTextures(view.drawableSize) }
        guard let accum, let bloomA, let bloomB, let model,
              let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable,
              let commands = queue.makeCommandBuffer()
        else { return }

        readVisuals(model)
        let scale = Float(view.window?.backingScaleFactor ?? 2)
        buildGeometry(dt: dt, pixelScale: scale)

        // 1. Persistence: fade the previous frame, then add this frame's light.
        let accumPass = MTLRenderPassDescriptor()
        accumPass.colorAttachments[0].texture = accum
        accumPass.colorAttachments[0].loadAction = needsClear ? .clear : .load
        accumPass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        accumPass.colorAttachments[0].storeAction = .store
        needsClear = false
        if let enc = commands.makeRenderCommandEncoder(descriptor: accumPass) {
            var decay = pow(Float(0.86), dt * 60)
            enc.setRenderPipelineState(fadePipeline)
            enc.setFragmentBytes(&decay, length: MemoryLayout<Float>.size, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
            if let buffer = makeBuffer(trace) {
                enc.setRenderPipelineState(linePipeline)
                enc.setVertexBuffer(buffer, offset: 0, index: 0)
                enc.drawPrimitives(type: .lineStrip, vertexStart: 0, vertexCount: trace.count)
            }
            if let buffer = makeBuffer(vertices) {
                enc.setRenderPipelineState(pointPipeline)
                enc.setVertexBuffer(buffer, offset: 0, index: 0)
                enc.drawPrimitives(type: .point, vertexStart: 0, vertexCount: vertices.count)
            }
            enc.endEncoding()
        }

        // 2. Bloom: half-res bright pass, then two separable Gaussian iterations.
        fullscreen(commands, pipeline: downsamplePipeline, target: bloomA, source: accum)
        let texel = SIMD2<Float>(1 / Float(bloomA.width), 1 / Float(bloomA.height))
        for spread: Float in [1.5, 3] {
            fullscreen(commands, pipeline: blurPipeline, target: bloomB, source: bloomA, step: SIMD2(texel.x * spread, 0))
            fullscreen(commands, pipeline: blurPipeline, target: bloomA, source: bloomB, step: SIMD2(0, texel.y * spread))
        }

        // 3. Composite with graticule, crosshair, vignette and tone mapping.
        if let enc = commands.makeRenderCommandEncoder(descriptor: pass) {
            var u = CompositeUniforms(
                focus: SIMD2(model.value(.focusX), model.value(.focusY)),
                focusAmount: model.value(.focusAmount) / 100,
                bloomStrength: 1.6,
                resolution: SIMD2(Float(view.drawableSize.width), Float(view.drawableSize.height)),
                margin: plot.margin,
                exposure: 1.4)
            enc.setRenderPipelineState(compositePipeline)
            enc.setFragmentTexture(accum, index: 0)
            enc.setFragmentTexture(bloomA, index: 1)
            enc.setFragmentBytes(&u, length: MemoryLayout<CompositeUniforms>.stride, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
            enc.endEncoding()
        }
        commands.present(drawable)
        commands.commit()
    }

    // MARK: Simulation

    private func readVisuals(_ model: PluginModel) {
        var frozen: ObjCBool = false
        let pool = slots.withUnsafeMutableBufferPointer { s in
            scopeL.withUnsafeMutableBufferPointer { l in
                scopeR.withUnsafeMutableBufferPointer { r in
                    model.audioUnit.readVisualSlots(s.baseAddress!, slotCount: s.count,
                                                    scopeLeft: l.baseAddress, scopeRight: r.baseAddress,
                                                    scopeCount: l.count, frozen: &frozen)
                }
            }
        }
        if model.poolCount != pool { model.poolCount = pool }
        if model.frozen != frozen.boolValue { model.frozen = frozen.boolValue }
        model.refreshClock()
    }

    private func buildGeometry(dt: Float, pixelScale: Float) {
        let snapshots = slots.map {
            SlotSnapshot(active: $0.active != 0, generation: $0.generation, triggerCount: $0.triggerCount,
                         position: SIMD2($0.x, $0.y), life: $0.life, clarity: $0.clarity, level: $0.level)
        }
        for event in tracker.update(snapshots) {
            switch event {
            case .born(let i):
                particles.burst(at: snapshots[i].position, color: SlotStyle.color(clarity: snapshots[i].clarity) * 0.6,
                                count: 14, speed: 0.1)
            case .triggered(let i):
                particles.burst(at: snapshots[i].position, color: SlotStyle.color(clarity: snapshots[i].clarity),
                                count: 64)
            case .died(let i):
                particles.burst(at: snapshots[i].position, color: SIMD3(repeating: 0.25), count: 10, speed: 0.05)
            }
        }
        for (i, s) in snapshots.enumerated() where s.active && s.level > 0.05 {
            emitCarry[i] += s.level * 90 * dt
            let n = Int(emitCarry[i])
            emitCarry[i] -= Float(n)
            if n > 0 { particles.emit(at: s.position, color: SlotStyle.color(clarity: s.clarity), count: n) }
        }
        particles.update(dt: dt)

        // Lissajous trace (X = L, Y = R) with slow automatic gain.
        var peak: Float = 0
        for i in scopeL.indices { peak = max(peak, abs(scopeL[i]), abs(scopeR[i])) }
        let targetGain = 0.42 / max(peak, 0.02)
        traceGain += (targetGain - traceGain) * min(1, dt * (targetGain < traceGain ? 8 : 1))
        trace.removeAll(keepingCapacity: true)
        let traceColor = SIMD4<Float>(0.05, 0.16, 0.13, 1)
        for i in scopeL.indices {
            let p = SIMD2(0.5 + scopeL[i] * traceGain, 0.5 + scopeR[i] * traceGain)
            trace.append(PointVertex(position: plot.toNDC(p), size: 1, color: traceColor))
        }

        vertices.removeAll(keepingCapacity: true)
        for s in snapshots where s.active {
            let life = min(s.life, 1)
            let color = SlotStyle.color(clarity: s.clarity)
            let intensity = 0.05 + 0.1 * life + 0.9 * s.level
            vertices.append(PointVertex(position: plot.toNDC(s.position),
                                        size: (5 + 8 * life + 14 * s.level) * pixelScale,
                                        color: SIMD4(color * intensity, 1)))
        }
        // Particles are re-drawn into the persistence buffer every frame, so they need less gain.
        particles.appendVertices(to: &vertices, plot: plot, pixelScale: pixelScale, gain: 0.35)
    }

    // MARK: Metal helpers

    private func makeTextures(_ size: CGSize) {
        let w = max(Int(size.width), 1), h = max(Int(size.height), 1)
        func texture(_ w: Int, _ h: Int) -> MTLTexture? {
            let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: Self.accumFormat, width: w, height: h, mipmapped: false)
            d.usage = [.renderTarget, .shaderRead]
            d.storageMode = .private
            return device.makeTexture(descriptor: d)
        }
        accum = texture(w, h)
        bloomA = texture(max(w / 2, 1), max(h / 2, 1))
        bloomB = texture(max(w / 2, 1), max(h / 2, 1))
        needsClear = true
    }

    private func makeBuffer(_ items: [PointVertex]) -> MTLBuffer? {
        guard !items.isEmpty else { return nil }
        return items.withUnsafeBytes { device.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared) }
    }

    private func fullscreen(_ commands: MTLCommandBuffer, pipeline: MTLRenderPipelineState,
                            target: MTLTexture, source: MTLTexture, step: SIMD2<Float>? = nil) {
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = target
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let enc = commands.makeRenderCommandEncoder(descriptor: pass) else { return }
        enc.setRenderPipelineState(pipeline)
        enc.setFragmentTexture(source, index: 0)
        if var step { enc.setFragmentBytes(&step, length: MemoryLayout<SIMD2<Float>>.size, index: 0) }
        enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        enc.endEncoding()
    }
}
