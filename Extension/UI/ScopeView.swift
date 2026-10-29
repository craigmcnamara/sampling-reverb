import MetalKit
import SwiftUI

/// The X/Y oscilloscope field. Drag to move the focus crosshair; scroll to change Focus amount;
/// double-click to toggle full screen.
struct ScopeView: NSViewRepresentable {
    @ObservedObject var model: PluginModel
    /// Stops drawing (and reading the audio unit's visual state, which has a single reader).
    var paused = false

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
        view.onDoubleClick = { model.scopeFullScreen.toggle() }
        return view
    }

    func updateNSView(_ nsView: ScopeMTKView, context: Context) {
        if nsView.isPaused != paused { nsView.isPaused = paused }
    }
}

final class ScopeMTKView: MTKView {
    var onPoint: ((CGPoint, CGSize) -> Void)?
    var onScroll: ((Float) -> Void)?
    var onDoubleClick: (() -> Void)?

    // Mouse input works without keyboard focus, and taking focus inside a plugin view swallows the
    // host's keys (Live's space bar) and has broken the remote view when focus moved on.
    override var acceptsFirstResponder: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2 {
            onDoubleClick?()
        } else {
            report(event)
        }
    }

    override func mouseDragged(with event: NSEvent) { report(event) }

    override func scrollWheel(with event: NSEvent) {
        onScroll?(Float(event.scrollingDeltaY))
    }

    private func report(_ event: NSEvent) {
        onPoint?(convert(event.locationInWindow, from: nil), bounds.size)
    }
}
