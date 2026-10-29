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
                .frame(width: 240)
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

struct AudioUnitView: NSViewControllerRepresentable {
    let viewController: NSViewController
    func makeNSViewController(context: Context) -> NSViewController { viewController }
    func updateNSViewController(_ nsViewController: NSViewController, context: Context) {}
}
