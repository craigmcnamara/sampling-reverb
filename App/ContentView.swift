import SwiftUI

struct ContentView: View {
    @ObservedObject var host: HostModel

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Picker("Source", selection: $host.source) {
                    ForEach(HostModel.Source.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .frame(width: 210)
                DevicePicker(label: "In", selection: $host.inputUID, devices: host.devices, input: true)
                DevicePicker(label: "Out", selection: $host.outputUID, devices: host.devices, input: false)
                Button("Open File…") { host.openFile() }
                if let name = host.fileName {
                    Text(name).foregroundStyle(.secondary).lineLimit(1)
                }
                Toggle("Bypass", isOn: $host.bypass)
                Spacer()
                Text(host.status)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .padding(10)
            Divider()
            if let vc = host.viewController {
                AudioUnitView(viewController: vc)
            } else {
                ProgressView(host.status).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 900, minHeight: 730)
        .task { host.load() }
    }
}

/// Chooses an audio device by UID; "System Default" (nil) follows the system setting.
struct DevicePicker: View {
    let label: String
    @Binding var selection: String?
    @ObservedObject var devices: AudioDevices
    let input: Bool

    var body: some View {
        let devices = input ? self.devices.inputs : self.devices.outputs
        Picker(label, selection: $selection) {
            Text("System Default").tag(String?.none)
            Divider()
            ForEach(devices) { Text($0.name).tag(String?.some($0.uid)) }
            if let selection, !devices.contains(where: { $0.uid == selection }) {
                Text("Unavailable device").tag(String?.some(selection))
            }
        }
        .frame(maxWidth: 220)
    }
}

struct AudioUnitView: NSViewControllerRepresentable {
    let viewController: NSViewController
    func makeNSViewController(context: Context) -> NSViewController { viewController }
    func updateNSViewController(_ nsViewController: NSViewController, context: Context) {}
}
