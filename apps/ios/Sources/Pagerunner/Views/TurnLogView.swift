import SwiftUI
import PagerunnerKit

/// Read-only list of `TurnLogEntry`s for the current thread's Scope.
struct TurnLogView: View {
    @Environment(AppState.self) private var appState

    private var entries: [TurnLogEntry] {
        // Newest first for the user-facing view.
        (appState.currentThread?.scope.turnLog ?? []).reversed()
    }

    var body: some View {
        List {
            if entries.isEmpty {
                ContentUnavailableView(
                    "No turns yet",
                    systemImage: "list.bullet.rectangle",
                    description: Text("The agent writes a summary at the end of each turn.")
                )
            } else {
                ForEach(entries, id: \.timestamp) { entry in
                    VStack(alignment: .leading, spacing: 6) {
                        if !entry.userGoal.isEmpty {
                            Text(entry.userGoal)
                                .font(.footnote.weight(.semibold))
                                .lineLimit(2)
                        }
                        Text(entry.summary)
                            .font(.caption)
                            .foregroundStyle(.primary)
                        HStack(spacing: 6) {
                            Text(entry.timestamp, style: .relative)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                            if !entry.touchedTabIds.isEmpty {
                                Text("·")
                                    .foregroundStyle(.tertiary)
                                Text("used \(entry.touchedTabIds.count) tab\(entry.touchedTabIds.count == 1 ? "" : "s")")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }
        .navigationTitle("Turn log")
        .navigationBarTitleDisplayMode(.inline)
    }
}
