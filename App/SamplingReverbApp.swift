import SwiftUI

@main
struct SamplingReverbApp: App {
    @StateObject private var host = HostModel()

    var body: some Scene {
        WindowGroup("Sampling Reverb") {
            ContentView(host: host)
        }
        .defaultSize(width: 1000, height: 750)
    }
}
