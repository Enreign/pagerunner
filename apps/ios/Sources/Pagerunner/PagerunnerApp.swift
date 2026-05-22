import SwiftUI
import PagerunnerKit

@main
struct PagerunnerApp: App {
    @State private var appState = AppState()

    init() {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        let b = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        PgrLog.app.info("launch v\(v, privacy: .public) (\(b, privacy: .public))")
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .preferredColorScheme(.dark)
        }
    }
}
