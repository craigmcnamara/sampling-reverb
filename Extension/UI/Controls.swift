import AppKit
import SwiftUI

enum Theme {
    static let background = Color(red: 0.035, green: 0.045, blue: 0.055)
    static let panel = Color(red: 0.07, green: 0.085, blue: 0.095)
    static let accent = Color(red: 0.35, green: 1.0, blue: 0.85)
    static let amber = Color(red: 1.0, green: 0.62, blue: 0.22)
    static let label = Color(white: 0.62)
    static let value = Color(white: 0.9)
}

/// Rotary knob: drag vertically (Shift = fine), double-click to reset.
struct Knob: View {
    @ObservedObject var model: PluginModel
    let id: SRParam
    let label: String
    var size: CGFloat = 46
    var tint: Color = Theme.accent

    @State private var dragStart: Float?

    private let sweep = 0.75  // fraction of a full turn

    var body: some View {
        let n = Double(model.normalized(id))
        let bipolar = model.mapping(id).isBipolar
        VStack(spacing: 4) {
            ZStack {
                Circle()
                    .fill(RadialGradient(colors: [Color(white: 0.2), Color(white: 0.08)],
                                         center: .topLeading, startRadius: 2, endRadius: size))
                    .overlay(Circle().stroke(Color(white: 0.25), lineWidth: 1))
                    .padding(6)
                Circle()
                    .trim(from: 0, to: sweep)
                    .stroke(Color(white: 0.18), style: StrokeStyle(lineWidth: 3, lineCap: .round))
                    .rotationEffect(.degrees(135))
                Circle()
                    .trim(from: bipolar ? min(n, 0.5) * sweep : 0, to: bipolar ? max(n, 0.5) * sweep : n * sweep)
                    .stroke(tint, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                    .rotationEffect(.degrees(135))
                    .shadow(color: tint.opacity(0.8), radius: 4)
                Capsule()
                    .fill(Theme.value)
                    .frame(width: 2, height: size * 0.28)
                    .offset(y: -size * 0.2)
                    .rotationEffect(.degrees(-135 + 270 * n))
            }
            .frame(width: size, height: size)
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { g in
                        let start = dragStart ?? model.normalized(id)
                        dragStart = start
                        let fine = NSEvent.modifierFlags.contains(.shift)
                        model.setNormalized(id, model.mapping(id).dragged(from: start, deltaY: -g.translation.height, fine: fine))
                    }
                    .onEnded { _ in dragStart = nil }
            )
            .onTapGesture(count: 2) { model.reset(id) }

            Text(label)
                .font(.system(size: 9, weight: .semibold, design: .monospaced))
                .foregroundStyle(Theme.label)
            Text(model.display(id))
                .font(.system(size: 9, weight: .regular, design: .monospaced))
                .foregroundStyle(Theme.value)
                .lineLimit(1)
        }
        .frame(width: size + 18)
        .help(label)
    }
}

/// Latching, footswitch-style toggle.
struct Footswitch: View {
    @ObservedObject var model: PluginModel
    let id: SRParam
    let label: String

    var body: some View {
        let on = model.value(id) >= 0.5
        Button {
            model.set(id, on ? 0 : 1)
        } label: {
            VStack(spacing: 6) {
                Circle()
                    .fill(on ? Theme.accent : Color(white: 0.15))
                    .frame(width: 10, height: 10)
                    .shadow(color: on ? Theme.accent : .clear, radius: 6)
                Circle()
                    .fill(RadialGradient(colors: [Color(white: 0.55), Color(white: 0.22)],
                                         center: .topLeading, startRadius: 1, endRadius: 30))
                    .frame(width: 34, height: 34)
                    .overlay(Circle().stroke(Color(white: 0.1), lineWidth: 2))
                Text(label)
                    .font(.system(size: 9, weight: .semibold, design: .monospaced))
                    .foregroundStyle(Theme.label)
            }
        }
        .buttonStyle(.plain)
    }
}

/// One preset slot in the bottom toolbar. A trailing dot means the knobs have moved since it was chosen.
struct PresetButton: View {
    let name: String
    let selected: Bool
    let modified: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(name.uppercased() + (modified ? " •" : ""))
                .font(.system(size: 10, weight: .semibold, design: .monospaced))
                .tracking(1)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .foregroundStyle(selected ? Theme.accent : Theme.label)
                .frame(maxWidth: .infinity, minHeight: 28)
                .background(
                    RoundedRectangle(cornerRadius: 6)
                        .fill(selected ? Theme.accent.opacity(0.12) : Color(white: 0.11))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(selected ? Theme.accent : Color(white: 0.2), lineWidth: 1)
                )
                .shadow(color: selected ? Theme.accent.opacity(0.6) : .clear, radius: 5)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(name)
    }
}
