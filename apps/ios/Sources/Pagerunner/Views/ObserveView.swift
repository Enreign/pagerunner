import SwiftUI
import PagerunnerKit

enum ObserveTab: String, CaseIterable {
    case network = "Network"
    case console = "Console"
    case recordings = "Recordings"
    case audit = "Audit"

    var icon: String {
        switch self {
        case .network: "network"
        case .console: "terminal"
        case .recordings: "record.circle"
        case .audit: "doc.text.magnifyingglass"
        }
    }
}

struct ObserveView: View {
    @Environment(AppState.self) private var appState

    @State private var selectedSubTab: ObserveTab = .network
    @State private var selectedSessionId: String?
    @State private var networkEntries: [NetworkLogEntry] = []
    @State private var consoleResult: ConsoleLogResult?

    var body: some View {
        VStack(spacing: 0) {
            picker
            Divider()

            if selectedSubTab == .network || selectedSubTab == .console {
                sessionPicker
                Divider()
            }

            Group {
                switch selectedSubTab {
                case .network: networkLogView
                case .console: consoleLogView
                case .recordings: recordingsView
                case .audit: auditView
                }
            }
        }
        .navigationTitle("Observe")
    }

    // MARK: - Picker

    private var picker: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                ForEach(ObserveTab.allCases, id: \.self) { tab in
                    Button {
                        selectedSubTab = tab
                    } label: {
                        Label(tab.rawValue, systemImage: tab.icon)
                            .font(.subheadline)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(
                                selectedSubTab == tab
                                    ? Color.accentColor.opacity(0.15)
                                    : Color(.tertiarySystemFill)
                            )
                            .foregroundStyle(
                                selectedSubTab == tab
                                    ? Color.accentColor
                                    : .secondary
                            )
                            .clipShape(Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 10)
        }
    }

    // MARK: - Session Picker

    private var sessionPicker: some View {
        HStack {
            Text("Session:")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Picker("Session", selection: $selectedSessionId) {
                Text("Select...").tag(nil as String?)
                ForEach(appState.sessions) { session in
                    Text("\(session.profile) (\(String(session.id.prefix(8))))")
                        .tag(session.id as String?)
                }
            }
            .pickerStyle(.menu)

            Spacer()

            if selectedSessionId != nil {
                Button {
                    Task { await fetchLogData() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color(.secondarySystemBackground))
    }

    // MARK: - Network Log

    private var networkLogView: some View {
        Group {
            if selectedSessionId == nil {
                ContentUnavailableView(
                    "Select a Session",
                    systemImage: "network",
                    description: Text("Choose a session above to view its network log.")
                )
            } else if networkEntries.isEmpty {
                ContentUnavailableView(
                    "No Network Activity",
                    systemImage: "network",
                    description: Text("No network requests have been logged yet.")
                )
            } else {
                List(networkEntries) { entry in
                    NetworkLogRow(entry: entry)
                }
                .listStyle(.plain)
            }
        }
        .onChange(of: selectedSessionId) {
            Task { await fetchLogData() }
        }
    }

    // MARK: - Console Log

    private var consoleLogView: some View {
        Group {
            if selectedSessionId == nil {
                ContentUnavailableView(
                    "Select a Session",
                    systemImage: "terminal",
                    description: Text("Choose a session above to view its console log.")
                )
            } else if (consoleResult?.consoleErrors.isEmpty ?? true) && (consoleResult?.exceptions.isEmpty ?? true) {
                ContentUnavailableView(
                    "No Console Output",
                    systemImage: "terminal",
                    description: Text("No console messages have been logged yet.")
                )
            } else {
                List {
                    if let errors = consoleResult?.consoleErrors, !errors.isEmpty {
                        Section("Console Errors") {
                            ForEach(errors) { entry in
                                consoleRow(level: entry.level, text: entry.text, timestampMs: entry.timestampMs)
                            }
                        }
                    }
                    if let exceptions = consoleResult?.exceptions, !exceptions.isEmpty {
                        Section("Exceptions") {
                            ForEach(exceptions) { entry in
                                consoleRow(level: "error", text: entry.text, timestampMs: entry.timestampMs)
                            }
                        }
                    }
                }
                .listStyle(.plain)
            }
        }
        .onChange(of: selectedSessionId) {
            Task { await fetchLogData() }
        }
    }

    private func consoleRow(level: String, text: String, timestampMs: UInt64) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: consoleIcon(for: level))
                .foregroundStyle(consoleColor(for: level))
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 4) {
                Text(text)
                    .font(.caption)
                    .monospaced()
                    .lineLimit(3)

                Text(formatTimestamp(timestampMs))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
    }

    private func consoleIcon(for level: String) -> String {
        switch level {
        case "error": "xmark.circle.fill"
        case "warning": "exclamationmark.triangle.fill"
        case "info": "info.circle.fill"
        default: "circle.fill"
        }
    }

    private func consoleColor(for level: String) -> Color {
        switch level {
        case "error": .red
        case "warning": .orange
        case "info": .blue
        default: .secondary
        }
    }

    private func formatTimestamp(_ ms: UInt64) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss.SSS"
        return formatter.string(from: date)
    }

    // MARK: - Recordings

    private var recordingsView: some View {
        Group {
            if appState.recordings.isEmpty {
                ContentUnavailableView(
                    "No Recordings",
                    systemImage: "record.circle",
                    description: Text("Session recordings will appear here.")
                )
            } else {
                List(appState.recordings) { recording in
                    recordingRow(recording)
                }
                .listStyle(.plain)
            }
        }
        .task {
            await appState.fetchRecordings()
        }
    }

    private func recordingRow(_ recording: Recording) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Image(systemName: "record.circle")
                    .foregroundStyle(.red)

                Text(recording.name ?? recording.flow ?? recording.recordingId.prefix(8).description)
                    .font(.headline)

                Spacer()

                if let durationMs = recording.durationMs {
                    Text(formatDuration(durationMs))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            HStack(spacing: 16) {
                Label(recording.format.uppercased(), systemImage: "film")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Label(recording.startedAt, systemImage: "clock")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if !recording.tags.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(recording.tags, id: \.self) { tag in
                            Text(tag)
                                .font(.caption2)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Color.accentColor.opacity(0.1))
                                .clipShape(Capsule())
                        }
                    }
                }
            }
        }
        .padding(.vertical, 4)
    }

    private func formatDuration(_ ms: UInt64) -> String {
        let seconds = ms / 1000
        let mins = seconds / 60
        let secs = seconds % 60
        if mins > 0 {
            return "\(mins)m \(secs)s"
        }
        return "\(secs)s"
    }

    // MARK: - Audit

    private var auditView: some View {
        ContentUnavailableView(
            "Audit Log",
            systemImage: "doc.text.magnifyingglass",
            description: Text("Audit log viewer coming soon.")
        )
    }

    // MARK: - Data Fetching

    private func fetchLogData() async {
        guard let sessionId = selectedSessionId,
              let client = appState.connection.apiClient else { return }

        switch selectedSubTab {
        case .network:
            do {
                let result = try await client.networkLog(
                    sessionId: sessionId,
                    limit: 100,
                    targetId: nil
                )
                networkEntries = result.entries
            } catch {
                networkEntries = []
            }
        case .console:
            do {
                consoleResult = try await client.consoleLog(
                    sessionId: sessionId,
                    targetId: nil
                )
            } catch {
                consoleResult = nil
            }
        default:
            break
        }
    }
}

#Preview {
    NavigationStack {
        ObserveView()
    }
    .environment(AppState())
}
