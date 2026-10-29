import AppKit
import SwiftUI

/// Header controls for trigger quantize: on/off, grid, tempo (drag or tap) and a beat light.
struct ClockStrip: View {
    @ObservedObject var model: PluginModel
    @ObservedObject var clock: ClockModel
    @State private var dragStart: Float?

    var body: some View {
        let on = model.value(.quantize) >= 0.5
        let fromHost = clock.display.fromHost
        HStack(spacing: 8) {
            Button { model.set(.quantize, on ? 0 : 1) } label: {
                HStack(spacing: 5) {
                    Circle()
                        .fill(on ? Theme.accent : Color(white: 0.2))
                        .frame(width: 7, height: 7)
                        .shadow(color: on ? Theme.accent : .clear, radius: 4)
                    Text("QUANTIZE")
                }
                .padding(.horizontal, 8)
                .frame(height: 22)
                .background(Capsule().fill(on ? Theme.accent.opacity(0.12) : Color(white: 0.11)))
                .overlay(Capsule().stroke(on ? Theme.accent : Color(white: 0.22), lineWidth: 1))
                .foregroundStyle(on ? Theme.accent : Theme.label)
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .help("Hold matched samples until the next grid point")

            Menu {
                ForEach(Array(model.valueNames(.grid).enumerated()), id: \.offset) { index, name in
                    Button(name) { model.set(.grid, Float(index)) }
                }
            } label: {
                Text(model.display(.grid))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.visible)
            .fixedSize()
            .foregroundStyle(on ? Theme.value : Theme.label)
            .help("Grid")

            BeatLight(display: clock.display)

            Text(String(format: "%5.1f", clock.display.tempo))
                .foregroundStyle(Theme.value)
                .frame(width: 42, alignment: .trailing)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 1)
                        .onChanged { g in
                            guard !fromHost else { return }
                            let start = dragStart ?? model.value(.tempo)
                            dragStart = start
                            let perPoint: Float = NSEvent.modifierFlags.contains(.shift) ? 0.05 : 0.5
                            model.set(.tempo, min(max(start - Float(g.translation.height) * perPoint, 40), 240))
                        }
                        .onEnded { _ in dragStart = nil }
                )
                .onTapGesture(count: 2) { if !fromHost { model.reset(.tempo) } }
                .help(fromHost ? "Tempo from the host" : "Drag to set the tempo (Shift = fine), double-click to reset")
            Text(fromHost ? "HOST" : "BPM")
                .foregroundStyle(fromHost ? Theme.accent : Theme.label)

            if !fromHost {
                Button("TAP") { model.tap() }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 8)
                    .frame(height: 22)
                    .background(RoundedRectangle(cornerRadius: 5).fill(Color(white: 0.11)))
                    .overlay(RoundedRectangle(cornerRadius: 5).stroke(Color(white: 0.22), lineWidth: 1))
                    .foregroundStyle(Theme.label)
                    .help("Tap tempo")
            }
        }
        .font(.system(size: 10, weight: .semibold, design: .monospaced))
    }
}

/// Flashes on each beat, brighter on the first beat of the bar.
struct BeatLight: View {
    let display: BeatDisplay

    var body: some View {
        let color = display.downbeat ? Theme.accent : Theme.amber
        Circle()
            .fill(display.lit ? color : Color(white: 0.18))
            .frame(width: 8, height: 8)
            .shadow(color: display.lit ? color : .clear, radius: 5)
    }
}
