import SwiftUI

struct PluginView: View {
    static let preferredSize = NSSize(width: 980, height: 650)

    @ObservedObject var model: PluginModel

    var body: some View {
        VStack(spacing: 12) {
            header
            HStack(alignment: .top, spacing: 12) {
                ScopeView(model: model)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color(white: 0.18), lineWidth: 1))
                samplerPanel
            }
            reverbPanel
            presetBar
        }
        .padding(14)
        .frame(minWidth: 860, minHeight: 590)
        .background(Theme.background)
    }

    private var header: some View {
        HStack {
            Text("SAMPLING REVERB")
                .font(.system(size: 13, weight: .heavy, design: .monospaced))
                .tracking(3)
                .foregroundStyle(Theme.accent)
            Spacer()
            Text("POOL \(model.poolCount)/\(Int(SR_MAX_SLOTS))")
                .font(.system(size: 10, weight: .medium, design: .monospaced))
                .foregroundStyle(Theme.label)
            if model.frozen {
                Text("FROZEN")
                    .font(.system(size: 10, weight: .bold, design: .monospaced))
                    .foregroundStyle(Theme.accent)
            }
        }
    }

    private var samplerPanel: some View {
        VStack(spacing: 10) {
            sectionTitle("SAMPLER")
            Grid(horizontalSpacing: 6, verticalSpacing: 10) {
                GridRow {
                    Knob(model: model, id: .retention, label: "RETENTION", tint: Theme.amber)
                    Knob(model: model, id: .sensitivity, label: "SENSE", tint: Theme.amber)
                }
                GridRow {
                    Knob(model: model, id: .match, label: "MATCH", tint: Theme.amber)
                    Knob(model: model, id: .harmony, label: "HARMONY", tint: Theme.amber)
                }
                GridRow {
                    Knob(model: model, id: .length, label: "LENGTH", tint: Theme.amber)
                    Knob(model: model, id: .density, label: "DENSITY", tint: Theme.amber)
                }
                GridRow {
                    Knob(model: model, id: .sampleLevel, label: "LEVEL", tint: Theme.amber)
                    Knob(model: model, id: .dryFeed, label: "DRY FEED", tint: Theme.amber)
                }
                GridRow {
                    Knob(model: model, id: .attack, label: "ATTACK", tint: Theme.amber)
                    Knob(model: model, id: .focusAmount, label: "FOCUS", tint: Theme.amber)
                }
            }
        }
        .padding(10)
        .frame(width: 170)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 10))
    }

    private var reverbPanel: some View {
        HStack(alignment: .bottom, spacing: 4) {
            Knob(model: model, id: .mix, label: "MIX")
            Knob(model: model, id: .decay, label: "DECAY")
            Knob(model: model, id: .size, label: "SIZE")
            Knob(model: model, id: .preDelay, label: "PRE")
            Knob(model: model, id: .tone, label: "TONE")
            Knob(model: model, id: .texture, label: "TEXTURE")
            Knob(model: model, id: .mod, label: "MOD")
            Knob(model: model, id: .shimmer, label: "SHIMMER")
            Knob(model: model, id: .interval, label: "INTERVAL")
            Knob(model: model, id: .sub, label: "SUB")
            Knob(model: model, id: .filter, label: "FILTER")
            Knob(model: model, id: .resonance, label: "RES")
            Spacer(minLength: 8)
            Footswitch(model: model, id: .freeze, label: "FREEZE")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 10))
    }

    private var presetBar: some View {
        HStack(spacing: 6) {
            ForEach(Array(model.presetNames.enumerated()), id: \.offset) { index, name in
                let selected = model.presetSelection.selected == index
                PresetButton(name: name, selected: selected, modified: selected && model.isPresetModified) {
                    model.applyPreset(index)
                }
            }
        }
        .padding(6)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 10))
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .bold, design: .monospaced))
            .tracking(2)
            .foregroundStyle(Theme.label)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}
