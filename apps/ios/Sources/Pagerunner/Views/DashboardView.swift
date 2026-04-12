import SwiftUI
import PagerunnerKit

struct DashboardView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                connectionStatusCard
                sessionSummaryCard
                profileGrid
                recentNotificationsCard
            }
            .padding()
        }
        .navigationTitle("Dashboard")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await appState.refresh() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
            }
        }
        .refreshable {
            await appState.refresh()
        }
    }

    // MARK: - Connection Status

    private var connectionStatusCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Circle()
                    .fill(appState.connection.isConnected ? .green : .red)
                    .frame(width: 10, height: 10)

                Text(appState.connection.isConnected ? "Connected" : "Disconnected")
                    .font(.headline)

                Spacer()

                if appState.connection.isConnected {
                    Text("\(appState.connection.host):\(appState.connection.port)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .monospaced()
                }
            }

            if appState.connection.isConnected {
                HStack {
                    Label("Daemon", systemImage: "server.rack")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)

                    Spacer()

                    if appState.isPolling {
                        Label("Polling", systemImage: "antenna.radiowaves.left.and.right")
                            .font(.caption2)
                            .foregroundStyle(.green)
                    }
                }
            }
        }
        .padding()
        .background(Color(.secondarySystemGroupedBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    // MARK: - Session Summary

    private var sessionSummaryCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Sessions")
                .font(.headline)

            HStack(spacing: 16) {
                summaryItem(
                    count: appState.sessions.count,
                    label: "Total",
                    color: .primary
                )

                Divider()
                    .frame(height: 40)

                summaryItem(
                    count: appState.aliveSessions.count,
                    label: "Alive",
                    color: .green
                )

                Divider()
                    .frame(height: 40)

                summaryItem(
                    count: appState.crashedSessions.count,
                    label: "Crashed",
                    color: .red
                )
            }
            .frame(maxWidth: .infinity)
        }
        .padding()
        .background(Color(.secondarySystemGroupedBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func summaryItem(count: Int, label: String, color: Color) -> some View {
        VStack(spacing: 4) {
            Text("\(count)")
                .font(.title.bold())
                .foregroundStyle(color)

            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Profile Grid

    private var profileGrid: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Profiles")
                .font(.headline)

            if appState.profiles.isEmpty {
                Text("No profiles configured")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 8)
            } else {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: 12) {
                    ForEach(appState.profiles) { profile in
                        profileCard(profile)
                    }
                }
            }
        }
        .padding()
        .background(Color(.secondarySystemGroupedBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func profileCard(_ profile: Profile) -> some View {
        Button {
            appState.selectedTab = .sessions
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Image(systemName: profileIcon(for: profile))
                        .font(.title3)
                        .foregroundStyle(.tint)

                    Spacer()

                    let count = appState.sessionsForProfile(profile.name).count
                    if count > 0 {
                        Text("\(count)")
                            .font(.caption.bold())
                            .padding(.horizontal, 8)
                            .padding(.vertical, 2)
                            .background(.tint.opacity(0.15))
                            .clipShape(Capsule())
                    }
                }

                Text(profile.name)
                    .font(.subheadline.bold())
                    .lineLimit(1)

                Text(profile.displayName)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(.tertiarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
    }

    private func profileIcon(for profile: Profile) -> String {
        switch profile.kind {
        case "agent": return "cpu"
        case "attached": return "link"
        default:
            if profile.name.lowercased().contains("work") {
                return "briefcase.fill"
            } else if profile.name.lowercased().contains("personal") {
                return "person.fill"
            } else {
                return "globe"
            }
        }
    }

    // MARK: - Recent Notifications

    private var recentNotificationsCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Recent Notifications")
                .font(.headline)

            if appState.recentNotifications.isEmpty {
                Text("No recent notifications")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 8)
            } else {
                ForEach(appState.recentNotifications) { notification in
                    HStack(spacing: 10) {
                        Image(systemName: notificationIcon(notification))
                            .foregroundStyle(notificationColor(notification))
                            .frame(width: 24)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(notification.title)
                                .font(.subheadline)
                                .lineLimit(1)

                            if let body = notification.body {
                                Text(body)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(2)
                            }
                        }

                        Spacer()

                        Text(Date(timeIntervalSince1970: Double(notification.createdAt) / 1_000_000), style: .relative)
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                    .padding(.vertical, 4)

                    if notification.id != appState.recentNotifications.last?.id {
                        Divider()
                    }
                }
            }
        }
        .padding()
        .background(Color(.secondarySystemGroupedBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func notificationIcon(_ notification: DaemonNotification) -> String {
        switch notification.level {
        case "error": "exclamationmark.triangle.fill"
        case "warning": "exclamationmark.circle.fill"
        default: "info.circle.fill"
        }
    }

    private func notificationColor(_ notification: DaemonNotification) -> Color {
        switch notification.level {
        case "error": .red
        case "warning": .orange
        default: .blue
        }
    }
}

#Preview {
    NavigationStack {
        DashboardView()
    }
    .environment(AppState())
}
