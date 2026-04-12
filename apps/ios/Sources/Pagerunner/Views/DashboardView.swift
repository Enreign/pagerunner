import SwiftUI
import PagerunnerKit

struct DashboardView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: Theme.Spacing.section) {
                connectionCard
                sessionStats
                profileSection
                notificationSection
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
        }
        .background(Color.operatorBackground)
        .navigationTitle("Dashboard")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await appState.refresh() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .accessibilityLabel("Refresh")
            }
        }
        .refreshable { await appState.refresh() }
    }

    // MARK: Connection card

    private var connectionCard: some View {
        Card {
            VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
                HStack(spacing: 10) {
                    StatusDot(state: appState.connection.isConnected ? .live : .error)
                    Text(appState.connection.isConnected ? "Connected" : "Disconnected")
                        .font(.headline)
                    Spacer()
                    if appState.isPolling && appState.connection.isConnected {
                        HStack(spacing: 6) {
                            StatusDot(state: .live, size: 6)
                            Text("LIVE")
                                .font(.statLabel)
                                .tracking(1.4)
                                .foregroundStyle(.accent)
                        }
                    }
                }

                if appState.connection.isConnected {
                    HStack(spacing: 6) {
                        Image(systemName: "server.rack")
                            .foregroundStyle(.secondary)
                        Text("\(appState.connection.host):\(appState.connection.port)")
                            .font(.mono)
                            .foregroundStyle(.primary)
                        Spacer()
                    }
                    .font(.subheadline)
                }
            }
        }
    }

    // MARK: Session stats

    private var sessionStats: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
            SectionLabel(text: "SESSIONS")
            HStack(spacing: Theme.Spacing.regular) {
                statTile(value: appState.sessions.count,         label: "Total",   tint: .primary)
                statTile(value: appState.aliveSessions.count,    label: "Alive",   tint: .accent)
                statTile(value: appState.crashedSessions.count,  label: "Crashed", tint: .red)
            }
        }
    }

    private func statTile(value: Int, label: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
            Text("\(value)")
                .font(.statNumber)
                .foregroundStyle(tint)
                .contentTransition(.numericText())
                .animation(.snappy, value: value)
            Text(label.uppercased())
                .font(.statLabel)
                .tracking(1.2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Theme.Spacing.loose)
        .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    // MARK: Profiles

    private var profileSection: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
            SectionLabel(text: "PROFILES · \(appState.profiles.count)")

            if appState.profiles.isEmpty {
                Card {
                    Text("No profiles configured. Add a [[profiles]] entry in config.toml on the daemon host.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            } else {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())],
                          spacing: Theme.Spacing.regular) {
                    ForEach(appState.profiles) { profile in
                        profileCard(profile)
                    }
                }
            }
        }
    }

    private func profileCard(_ profile: Profile) -> some View {
        Button {
            appState.selectedTab = .sessions
        } label: {
            VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
                HStack {
                    Image(systemName: profileIcon(for: profile))
                        .font(.title3)
                        .foregroundStyle(.accent)
                    Spacer()
                    let count = appState.sessionsForProfile(profile.name).count
                    if count > 0 {
                        Text("\(count)")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.accent)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 2)
                            .background(.accent.opacity(0.15), in: Capsule())
                    }
                }
                Text(profile.name)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .foregroundStyle(.primary)
                Text(profile.displayName)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Theme.Spacing.regular + 2)
            .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card - 2, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private func profileIcon(for profile: Profile) -> String {
        switch profile.kind {
        case "agent":    return "cpu"
        case "attached": return "link"
        default:
            let n = profile.name.lowercased()
            if n.contains("work") { return "briefcase.fill" }
            if n.contains("personal") { return "person.fill" }
            return "globe"
        }
    }

    // MARK: Notifications

    private var notificationSection: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
            SectionLabel(text: "ACTIVITY")

            Card(padding: Theme.Spacing.regular) {
                if appState.recentNotifications.isEmpty {
                    HStack {
                        Image(systemName: "tray")
                            .foregroundStyle(.secondary)
                        Text("No recent activity")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                        Spacer()
                    }
                    .padding(Theme.Spacing.tight)
                } else {
                    VStack(spacing: 0) {
                        ForEach(appState.recentNotifications) { notification in
                            notificationRow(notification)
                            if notification.id != appState.recentNotifications.last?.id {
                                Divider().padding(.leading, 34)
                            }
                        }
                    }
                }
            }
        }
    }

    private func notificationRow(_ n: DaemonNotification) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: notificationIcon(n))
                .foregroundStyle(notificationColor(n))
                .frame(width: 24)
                .padding(.top, 2)

            VStack(alignment: .leading, spacing: 2) {
                Text(n.title)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                if let body = n.body {
                    Text(body)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }

            Spacer()

            Text(Date(timeIntervalSince1970: Double(n.createdAt) / 1_000_000), style: .relative)
                .font(.monoCaption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, Theme.Spacing.tight)
    }

    private func notificationIcon(_ n: DaemonNotification) -> String {
        switch n.level {
        case "error":   "exclamationmark.triangle.fill"
        case "warning": "exclamationmark.circle.fill"
        default:        "info.circle.fill"
        }
    }

    private func notificationColor(_ n: DaemonNotification) -> Color {
        switch n.level {
        case "error":   .red
        case "warning": .orange
        default:        .accent
        }
    }
}

#Preview {
    NavigationStack {
        DashboardView()
    }
    .environment(AppState())
}
