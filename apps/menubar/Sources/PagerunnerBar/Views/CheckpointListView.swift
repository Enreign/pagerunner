import SwiftUI
import PagerunnerCore

struct CheckpointListView: View {
    @Bindable var appState: AppState
    let profileName: String
    /// ID of the first alive session for this profile — used as the restore target.
    /// Nil if no sessions are running (Restore button is disabled).
    var activeSessionId: String? {
        appState.sessionsFor(profile: profileName).first(where: { $0.status == .alive })?.id
    }
    @State private var isExpanded = false

    private var checkpoints: [Checkpoint] { appState.checkpointsFor(profile: profileName) }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                isExpanded.toggle()
            } label: {
                HStack {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 9))
                        .foregroundStyle(.secondary)
                    Text("Saved sessions")
                        .font(.system(size: 11, weight: .medium))
                    Spacer()
                    Text("\(checkpoints.count)")
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                }
            }
            .buttonStyle(.plain)

            if isExpanded {
                if checkpoints.isEmpty {
                    Text("No saved checkpoints")
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                        .padding(.leading, 14)
                } else {
                    ForEach(checkpoints) { checkpoint in
                        CheckpointRow(
                            checkpoint: checkpoint,
                            profileName: profileName,
                            sessionId: activeSessionId,
                            appState: appState
                        )
                    }
                }
            }
        }
    }
}

struct CheckpointRow: View {
    let checkpoint: Checkpoint
    let profileName: String
    let sessionId: String?   // nil = no active session for this profile
    @Bindable var appState: AppState

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(checkpoint.name)
                    .font(.system(size: 11, weight: .medium))

                // Origin preview
                HStack(spacing: 4) {
                    ForEach(checkpoint.origins.prefix(3), id: \.self) { origin in
                        Text(origin)
                            .font(.system(size: 9))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Color.gray.opacity(0.12))
                            .cornerRadius(3)
                    }
                    if checkpoint.origins.count > 3 {
                        Text("+\(checkpoint.origins.count - 3)")
                            .font(.system(size: 9))
                            .foregroundStyle(.tertiary)
                    }
                }

                Text("\(checkpoint.tabCount) tabs · \(formatTimestamp(checkpoint.savedAt))")
                    .font(.system(size: 9))
                    .foregroundStyle(.tertiary)
            }

            Spacer()

            VStack(spacing: 4) {
                // Restore button — disabled if no active session or daemon stopped
                Button("Restore") {
                    guard let sid = sessionId else { return }
                    // TODO (Task 10): call restore_session_checkpoint via DaemonClient
                    _ = sid
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.mini)
                .disabled(sessionId == nil || appState.daemonStatus == .stopped)
                .help(sessionId == nil ? "Open a session first to restore" : "Restore checkpoint")

                // Delete button
                Button {
                    // TODO (Task 10): call delete_session_checkpoint via DaemonClient
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 9))
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
        }
        .padding(.leading, 14)
        .padding(.vertical, 4)
    }

    private func formatTimestamp(_ unix: Int) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unix))
        let fmt = RelativeDateTimeFormatter()
        fmt.unitsStyle = .abbreviated
        return fmt.localizedString(for: date, relativeTo: Date())
    }
}
