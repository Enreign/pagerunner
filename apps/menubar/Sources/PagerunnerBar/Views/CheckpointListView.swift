import SwiftUI
import PagerunnerCore

struct CheckpointListView: View {
    @Bindable var appState: AppState
    let profileName: String
    var activeSessionId: String? {
        appState.sessionsFor(profile: profileName).first(where: { $0.status == .alive })?.id
    }
    @State private var isExpanded = false

    private var checkpoints: [Checkpoint] { appState.checkpointsFor(profile: profileName) }

    @ViewBuilder
    var body: some View {
        if !checkpoints.isEmpty {
            checkpointSection
        }
    }

    private var checkpointSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Toggle header (spec: ckpt-toggle)
            Button {
                isExpanded.toggle()
            } label: {
                HStack(spacing: 5) {
                    Text(isExpanded ? "▸" : "▸")
                        .font(.system(size: 9))
                        .foregroundColor(.secondary)
                        .rotationEffect(isExpanded ? .degrees(90) : .degrees(0))
                        .animation(.easeInOut(duration: 0.18), value: isExpanded)
                    Text("Snapshots")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(.secondary)
                        .textCase(.uppercase)
                        .tracking(0.4)
                    Spacer()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 5)
                .background(Color.primary.opacity(0.04))
            }
            .buttonStyle(.plain)
            .overlay(alignment: .top) {
                Rectangle().fill(Color.primary.opacity(0.1)).frame(height: 0.5)
            }

            if isExpanded {
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

struct CheckpointRow: View {
    let checkpoint: Checkpoint
    let profileName: String
    let sessionId: String?
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var daemon

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            // Top line: name + age + restore + delete (spec: cktop)
            HStack(spacing: 5) {
                Text(checkpoint.name)
                    .font(.system(size: 12, weight: .medium))
                Spacer()
                Text(formatTimestamp(checkpoint.savedAt))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)

                Button("Restore") {
                    guard let sid = sessionId else { return }
                    Task { @MainActor in
                        _ = try? await daemon.call(tool: "restore_session_checkpoint", args: ["session_id": sid, "checkpoint_id": checkpoint.id])
                    }
                }
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                .buttonStyle(.plain)
                .disabled(sessionId == nil || appState.daemonStatus == .stopped)

                Button {
                    Task { @MainActor in
                        _ = try? await daemon.call(tool: "delete_session_checkpoint", args: ["profile": profileName, "checkpoint_id": checkpoint.id])
                    }
                } label: {
                    Text("✕")
                        .font(.system(size: 9))
                        .foregroundColor(.secondary)
                        .frame(width: 14, height: 14)
                        .clipShape(Circle())
                }
                .buttonStyle(.plain)
            }

            // Tab info (spec: cktabs 11px #888)
            Text(checkpoint.origins.prefix(3).joined(separator: " · ") +
                 (checkpoint.origins.count > 3 ? " +\(checkpoint.origins.count - 3)" : "") +
                 " — \(checkpoint.tabCount) tabs")
                .font(.system(size: 11))
                .foregroundColor(.secondary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 5)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 0.5)
        }
    }

    private func formatTimestamp(_ unix: Int) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unix))
        let fmt = RelativeDateTimeFormatter()
        fmt.unitsStyle = .abbreviated
        return fmt.localizedString(for: date, relativeTo: Date())
    }
}
