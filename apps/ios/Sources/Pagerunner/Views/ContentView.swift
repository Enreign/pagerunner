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
            if !appState.connection.token.isEmpty {
                await appState.connection.connect()
                if appState.connection.isConnected {
                    appState.startPolling()
                }
            }
        }
    }

    private var authenticatedRoot: some View {
        @Bindable var state = appState
        return TabView(selection: $state.selectedTab) {
            Tab(AppTab.dashboard.title, systemImage: AppTab.dashboard.icon, value: .dashboard) {
                NavigationStack { DashboardView() }
            }
            Tab(AppTab.sessions.title, systemImage: AppTab.sessions.icon, value: .sessions) {
                NavigationStack { SessionListView() }
            }
            Tab(AppTab.agent.title, systemImage: AppTab.agent.icon, value: .agent) {
                NavigationStack { AgentView() }
            }
            Tab(AppTab.observe.title, systemImage: AppTab.observe.icon, value: .observe) {
                NavigationStack { ObserveView() }
            }
            Tab(AppTab.settings.title, systemImage: AppTab.settings.icon, value: .settings) {
                NavigationStack { SettingsView() }
            }
        }
    }
}

#Preview {
    ContentView()
        .environment(AppState())
}
