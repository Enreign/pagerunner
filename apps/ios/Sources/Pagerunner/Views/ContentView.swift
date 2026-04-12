import SwiftUI
import PagerunnerKit

struct ContentView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        Group {
            if appState.connection.isConnected {
                authenticatedRoot
                    .transition(.opacity.combined(with: .scale(scale: 1.02)))
            } else {
                OnboardingView()
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
            }
        }
        .animation(.spring(duration: 0.5, bounce: 0.15), value: appState.connection.isConnected)
        .task {
            appState.connection.loadSettings()
            guard !appState.connection.host.isEmpty else { return }
            // Probe first — we might not need a token (tailscale mode).
            let mode = await appState.connection.probeAuthMode()
            if mode == .tailscale || !appState.connection.token.isEmpty {
                await appState.connection.connect()
                if appState.connection.isConnected {
                    appState.startPolling()
                }
            }
        }
    }

    private var authenticatedRoot: some View {
        NavigationStack { ChatView() }
    }
}

#Preview {
    ContentView()
        .environment(AppState())
}
