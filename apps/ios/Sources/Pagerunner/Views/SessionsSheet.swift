import SwiftUI
import PagerunnerKit

struct SessionsSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    var onOpenInspector: (ChatView.InspectorContext) -> Void

    var body: some View {
        NavigationStack {
            List {
                summaryRow
                    .listRowBackground(Color.operatorCard)

                Section("Sessions") {
                    if appState.sessions.isEmpty {
                        Text("No active sessions")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .listRowBackground(Color.operatorCard)
                    } else {
                        ForEach(appState.sessions) { session in
                            sessionRow(session)
                                .listRowBackground(Color.operatorCard)
                        }
                    }
                }

                Section("Profiles") {
                    ForEach(appState.profiles) { profile in
                        profileRow(profile)
                            .listRowBackground(Color.operatorCard)
                    }
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color.operatorBackground)
            .navigationTitle("Sessions")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private var summaryRow: some View {
        HStack(spacing: Theme.Spacing.loose) {
            summaryTile(appState.sessions.count, "Total", tint: .primary)
            summaryTile(appState.aliveSessions.count, "Alive", tint: .accent)
            summaryTile(appState.crashedSessions.count, "Crashed", tint: .red)
        }
    }

    private func summaryTile(_ value: Int, _ label: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("\(value)")
                .font(.title2.bold())
                .foregroundStyle(tint)
            Text(label.uppercased())
                .font(.statLabel)
                .tracking(1.2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func sessionRow(_ session: Session) -> some View {
        Button {
            onOpenInspector(.init(sessionId: session.id, targetId: nil))
        } label: {
            HStack(spacing: Theme.Spacing.regular) {
                StatusDot(state: session.status == .alive ? .live : .error)
                VStack(alignment: .leading, spacing: 2) {
                    Text(session.profile)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.primary)
                    Text(session.displayName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer()
                let tabCount = appState.tabs[session.id]?.count ?? 0
                if tabCount > 0 {
                    Text("\(tabCount) tab\(tabCount == 1 ? "" : "s")")
                        .font(.monoCaption)
                        .foregroundStyle(.secondary)
                }
                Image(systemName: "chevron.right")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .buttonStyle(.plain)
    }

    private func profileRow(_ profile: Profile) -> some View {
        let sessionCount = appState.sessionsForProfile(profile.name).count
        return HStack(spacing: Theme.Spacing.regular) {
            Image(systemName: "person.crop.circle")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(profile.name)
                    .font(.subheadline.weight(.medium))
                Text(profile.displayName)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            if sessionCount > 0 {
                Text("\(sessionCount)")
                    .font(.caption.bold())
                    .foregroundStyle(.accent)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(.accent.opacity(0.15), in: Capsule())
            }
        }
    }
}
