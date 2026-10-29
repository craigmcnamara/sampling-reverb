import AppKit
import Combine
import SwiftUI

/// Shows the scope full screen in its own window, on the display the plugin is on, while
/// `model.scopeFullScreen` is set. Esc, double-click or the exit button closes it.
final class ScopeWindowController: NSObject, NSWindowDelegate {
    private let model: PluginModel
    private let screen: () -> NSScreen?
    private var window: NSWindow?
    private var subscription: AnyCancellable?

    init(model: PluginModel, screen: @escaping () -> NSScreen?) {
        self.model = model
        self.screen = screen
        super.init()
        subscription = model.$scopeFullScreen.removeDuplicates().sink { [weak self] on in
            on ? self?.open() : self?.close()
        }
    }

    deinit { window?.close() }

    private func open() {
        guard window == nil, let screen = screen() ?? NSScreen.main else { return }
        let window = FullScreenWindow(contentRect: screen.frame, styleMask: [.borderless],
                                      backing: .buffered, defer: false)
        window.level = .mainMenu + 1  // above the menu bar and Dock
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.isReleasedWhenClosed = false
        window.backgroundColor = .black
        window.hasShadow = false
        window.delegate = self
        window.onExit = { [weak self] in self?.model.scopeFullScreen = false }
        window.contentView = NSHostingView(rootView: FullScreenScopeView(model: model, clock: model.clock))
        window.setFrame(screen.frame, display: true)
        window.makeKeyAndOrderFront(nil)
        self.window = window
    }

    private func close() {
        guard let window else { return }
        self.window = nil
        window.close()
    }

    func windowWillClose(_ notification: Notification) {
        window = nil
        if model.scopeFullScreen { model.scopeFullScreen = false }
    }
}

private final class FullScreenWindow: NSWindow {
    var onExit: (() -> Void)?

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    override func cancelOperation(_ sender: Any?) { onExit?() }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 || event.charactersIgnoringModifiers == "f" {  // Esc or F
            onExit?()
        } else {
            super.keyDown(with: event)
        }
    }
}

private struct FullScreenScopeView: View {
    @ObservedObject var model: PluginModel
    @ObservedObject var clock: ClockModel

    var body: some View {
        ZStack(alignment: .top) {
            ScopeView(model: model)
            HStack(spacing: 16) {
                Text("SAMPLING REVERB")
                    .font(.system(size: 13, weight: .heavy, design: .monospaced))
                    .tracking(3)
                    .foregroundStyle(Theme.accent.opacity(0.7))
                Text("POOL \(model.poolCount)/\(Int(SR_MAX_SLOTS))")
                if model.frozen { Text("FROZEN").foregroundStyle(Theme.accent) }
                Spacer()
                if model.value(.quantize) >= 0.5 { BeatLight(display: clock.display) }
                Text(String(format: "%.1f BPM", clock.display.tempo))
                Button { model.scopeFullScreen = false } label: {
                    Image(systemName: "arrow.down.right.and.arrow.up.left")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("Exit full screen (Esc)")
            }
            .font(.system(size: 11, weight: .medium, design: .monospaced))
            .foregroundStyle(Theme.label.opacity(0.8))
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
        .background(Color.black)
        .ignoresSafeArea()
    }
}
