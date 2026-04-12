import SwiftUI
import PagerunnerKit

struct ContentView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        @Bindable var state = appState

        ZStack {
            TabView(selection: $state.selectedTab) {
                Tab(AppTab.dashboard.title, systemImage: AppTab.dashboard.icon, value: .dashboard) {
                    NavigationStack {
                        DashboardView()
                    }
                }

                Tab(AppTab.sessions.title, systemImage: AppTab.sessions.icon, value: .sessions) {
                    NavigationStack {
                        SessionListView()
                    }
                }

                Tab(AppTab.agent.title, systemImage: AppTab.agent.icon, value: .agent) {
                    NavigationStack {
                        AgentView()
                    }
                }

                Tab(AppTab.observe.title, systemImage: AppTab.observe.icon, value: .observe) {
                    NavigationStack {
                        ObserveView()
                    }
                }

                Tab(AppTab.settings.title, systemImage: AppTab.settings.icon, value: .settings) {
                    NavigationStack {
                        SettingsView()
                    }
                }
            }

            if !appState.connection.isConnected && appState.selectedTab != .settings {
                ConnectOverlay { appState.selectedTab = .settings }
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
            }
        }
        .animation(.spring(duration: 0.35, bounce: 0.15), value: appState.connection.isConnected)
        .animation(.spring(duration: 0.35, bounce: 0.15), value: appState.selectedTab)
        .task {
            appState.connection.loadSettings()
            if !appState.connection.token.isEmpty {
                await appState.connection.connect()
                if appState.connection.isConnected {
                    appState.startPolling()
                }
            }
        }
    }

}

#Preview {
    ContentView()
        .environment(AppState())
}
