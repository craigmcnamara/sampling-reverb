import MetalKit
import SwiftUI

/// The X/Y oscilloscope field. Drag to move the focus crosshair; scroll to change Focus amount.
struct ScopeView: NSViewRepresentable {
    @ObservedObject var model: PluginModel

    final class Coordinator {
        var renderer: ScopeRenderer?
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ScopeMTKView {
        let view = ScopeMTKView(frame: .zero, device: MTLCreateSystemDefaultDevice())
        view.preferredFramesPerSecond = 60
        let renderer = ScopeRenderer(view: view, model: model)
        view.delegate = renderer
        context.coordinator.renderer = renderer

        let plot = PlotSpace()
        let model = self.model
        view.onPoint = { point, size in
            let p = plot.fromView(point, size: size)
            model.set(.focusX, p.x)
            model.set(.focusY, p.y)
        }
        view.onScroll = { delta in
            model.set(.focusAmount, min(max(model.value(.focusAmount) + delta * 0.5, 0), 100))
        }
        return view
    }

    func updateNSView(_ nsView: ScopeMTKView, context: Context) {}
}

final class ScopeMTKView: MTKView {
    var onPoint: ((CGPoint, CGSize) -> Void)?
    var onScroll: ((Float) -> Void)?

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) { report(event) }
    override func mouseDragged(with event: NSEvent) { report(event) }

    override func scrollWheel(with event: NSEvent) {
        onScroll?(Float(event.scrollingDeltaY))
    }

    private func report(_ event: NSEvent) {
        onPoint?(convert(event.locationInWindow, from: nil), bounds.size)
    }
}
