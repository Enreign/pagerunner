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
                notConnectedOverlay
            }
        }
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

    private var notConnectedOverlay: some View {
        VStack(spacing: 16) {
            Spacer()

            VStack(spacing: 12) {
                Image(systemName: "wifi.slash")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)

                Text("Not Connected")
                    .font(.title2.bold())

                Text("Connect to a Pagerunner daemon in Settings to get started.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)

                Button {
                    appState.selectedTab = .settings
                } label: {
                    Label("Go to Settings", systemImage: "gear")
                        .font(.headline)
                        .padding(.horizontal, 24)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
                .padding(.top, 4)
            }
            .padding(32)
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 20))
            .padding(.horizontal, 24)

            Spacer()
            Spacer()
        }
    }
}

#Preview {
    ContentView()
        .environment(AppState())
}
