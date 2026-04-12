import SwiftUI
import PagerunnerKit

struct SessionListView: View {
    @Environment(AppState.self) private var appState
    @State private var searchText = ""

    var body: some View {
        List {
            ForEach(groupedSessions, id: \.key) { profileName, sessions in
                Section(profileName) {
                    ForEach(sessions) { session in
                        NavigationLink(value: session) {
                            SessionRow(
                                session: session,
                                tabCount: appState.tabs[session.id]?.count ?? 0
                            )
                        }
                        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                            Button(role: .destructive) {
                                Task {
                                    try? await appState.closeSession(session.id)
                                }
                            } label: {
                                Label("Close", systemImage: "xmark.circle")
                            }
                        }
                    }
                }
            }

            if filteredSessions.isEmpty {
                ContentUnavailableView(
                    "No Sessions",
                    systemImage: "macwindow",
                    description: Text("Open a session from a profile to get started.")
                )
            }
        }
        .navigationTitle("Sessions")
        .navigationDestination(for: Session.self) { session in
            SessionDetailView(session: session)
        }
        .searchable(text: $searchText, prompt: "Search sessions")
        .refreshable {
            await appState.refresh()
        }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    ForEach(appState.profiles) { profile in
                        Button {
                            Task { try? await appState.openSession(profile: profile.name) }
                        } label: {
                            Label(profile.name, systemImage: "plus.rectangle")
                        }
                    }
                } label: {
                    Image(systemName: "plus")
                }
                .disabled(appState.profiles.isEmpty)
            }
        }
    }

    // MARK: - Filtering & Grouping

    private var filteredSessions: [Session] {
        if searchText.isEmpty {
            return appState.sessions
        }
        return appState.sessions.filter { session in
            session.profile.localizedCaseInsensitiveContains(searchText)
                || session.id.localizedCaseInsensitiveContains(searchText)
        }
    }

    private var groupedSessions: [(key: String, value: [Session])] {
        Dictionary(grouping: filteredSessions, by: \.profile)
            .sorted { $0.key < $1.key }
    }
}

// MARK: - Session Row

struct SessionRow: View {
    let session: Session
    let tabCount: Int

    var body: some View {
        HStack(spacing: 12) {
            StatusBadge(status: session.status)

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(session.profile)
                        .font(.headline)

                    if session.stealth {
                        Image(systemName: "eye.slash.fill")
                            .font(.caption)
                            .foregroundStyle(.purple)
                    }
                }

                Text("Session \(String(session.id.prefix(8)))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospaced()
            }

            Spacer()

            if tabCount > 0 {
                Text("\(tabCount)")
                    .font(.caption.bold())
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color(.tertiarySystemFill))
                    .clipShape(Capsule())
            }
        }
        .padding(.vertical, 4)
    }
}

#Preview {
    NavigationStack {
        SessionListView()
    }
    .environment(AppState())
}
