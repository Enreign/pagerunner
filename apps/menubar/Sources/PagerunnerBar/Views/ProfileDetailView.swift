import SwiftUI
import PagerunnerCore

struct ProfileDetailView: View {
    @Bindable var appState: AppState
    let profileName: String
    let controller: StatusItemController
    @Environment(\.daemonClient) private var daemon

    private var profile: Profile? { appState.profiles.first { $0.name == profileName } }
    private var sessions: [Session] { appState.sessionsFor(profile: profileName) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Back button + title
            HStack {
                Button {
                    appState.navigation = .overview
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "chevron.left")
                        Text("Overview")
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(Color.accentColor)
                }
                .buttonStyle(.plain)

                Spacer()

                Text(profile?.displayName ?? profileName)
                    .font(.system(size: 12, weight: .semibold))
            }

            // Session blocks
            if sessions.isEmpty {
                Text("No sessions")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 8)
            } else {
                ForEach(sessions) { session in
                    SessionBlockView(
                        session: session,
                        tabs: appState.tabsFor(session: session.id),
                        appState: appState,
                        controller: controller
                    )
                }
            }

            // Open new session button
            Button {
                Task { @MainActor in
                    _ = try? await daemon.call(tool: "open_session", args: ["profile": profileName])
                }
            } label: {
                Label("Open new session", systemImage: "plus")
                    .font(.system(size: 11))
            }
            .buttonStyle(.bordered)
            .disabled(appState.daemonStatus == .stopped)

            // Saved checkpoints
            CheckpointListView(appState: appState, profileName: profileName)
        }
    }
}
