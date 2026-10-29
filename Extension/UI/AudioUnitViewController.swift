import CoreAudioKit
import SwiftUI

/// Extension principal class: creates the audio unit and hosts its SwiftUI interface.
public class AudioUnitViewController: AUViewController, AUAudioUnitFactory {
    private var audioUnit: SRAudioUnit?
    private var hostingView: NSView?
    private var scopeWindow: ScopeWindowController?

    public override func loadView() {
        view = NSView(frame: NSRect(origin: .zero, size: PluginView.preferredSize))
        preferredContentSize = PluginView.preferredSize
    }

    public override func viewDidLoad() {
        super.viewDidLoad()
        attachInterface()
    }

    public func createAudioUnit(with componentDescription: AudioComponentDescription) throws -> AUAudioUnit {
        let unit = try SRAudioUnit(componentDescription: componentDescription, options: [])
        audioUnit = unit
        DispatchQueue.main.async { self.attachInterface() }
        return unit
    }

    private func attachInterface() {
        guard hostingView == nil, isViewLoaded, let audioUnit else { return }
        let model = PluginModel(audioUnit: audioUnit)
        let hosting = PassiveHostingView(rootView: PluginView(model: model))
        hosting.frame = view.bounds
        hosting.autoresizingMask = [.width, .height]
        view.addSubview(hosting)
        hostingView = hosting
        scopeWindow = ScopeWindowController(model: model) { [weak self] in self?.view.window?.screen }
    }
}

/// Hosts the plugin UI without ever taking keyboard focus, so keys stay with the host (Live's space
/// bar, shortcuts) and the remote view never has a focused view of ours when the host's focus moves.
/// A focused view inside the plugin made ViewBridge drop the view (left white) after a focus change.
final class PassiveHostingView<Content: View>: NSHostingView<Content> {
    override var acceptsFirstResponder: Bool { false }
}
