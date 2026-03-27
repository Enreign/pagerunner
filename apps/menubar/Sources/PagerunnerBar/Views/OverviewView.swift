import SwiftUI
import PagerunnerCore

/// Default home screen: scrollable list of all profiles, two sections.
struct OverviewView: View {
    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if !appState.personalProfiles.isEmpty {
                Section {
                    ForEach(appState.personalProfiles) { profile in
                        ProfileRow(profile: profile, appState: appState)
                    }
                } header: {
                    Text("Your profiles")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .textCase(.uppercase)
                }
            }

            if !appState.agentProfiles.isEmpty {
                Section {
                    ForEach(appState.agentProfiles) { profile in
                        ProfileRow(profile: profile, appState: appState)
                    }
                } header: {
                    Text("Agent profiles")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .textCase(.uppercase)
                }
            }

            if appState.profiles.isEmpty {
                Text("No profiles configured")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.top, 20)
            }
        }
    }
}

struct ProfileRow: View {
    let profile: Profile
    @Bindable var appState: AppState

    private var sessions: [Session] { appState.sessionsFor(profile: profile.name) }
    private var aliveSessions: [Session] { sessions.filter { $0.status == .alive } }
    private var topURL: String? {
        aliveSessions.first.flatMap { appState.tabsFor(session: $0.id).first?.url }
    }

    var body: some View {
        Button {
            appState.navigation = .profile(profile.name)
        } label: {
            HStack(spacing: 10) {
                // Profile icon
                profileIcon

                // Name + top URL
                VStack(alignment: .leading, spacing: 2) {
                    Text(profile.displayName)
                        .font(.system(size: 12, weight: .medium))
                    Text(topURL ?? (aliveSessions.isEmpty ? "No open sessions" : "Loading…"))
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Spacer()

                // Session count badge
                if !sessions.isEmpty {
                    Text("\(aliveSessions.count)")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundColor(aliveSessions.isEmpty ? .secondary : .white)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(aliveSessions.isEmpty ? Color.gray.opacity(0.2) : Color.green)
                        .cornerRadius(10)
                }

                Image(systemName: "chevron.right")
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var profileIcon: some View {
        let isAgent = profile.kind == "agent"
        return ZStack {
            Circle()
                .fill(isAgent ? Color.gray.opacity(0.25) : profileColor(profile.name))
                .frame(width: 32, height: 32)
            Text(String(profile.displayName.prefix(1)).uppercased())
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(isAgent ? .secondary : .white)
        }
    }

    private func profileColor(_ name: String) -> Color {
        let colors: [Color] = [.blue, .purple, .pink, .orange, .teal, .indigo, .cyan, .mint]
        let idx = abs(name.hashValue) % colors.count
        return colors[idx]
    }
}
