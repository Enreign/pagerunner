import SwiftUI
import PagerunnerKit

struct ThreadsDrawer: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                ForEach(appState.threads) { thread in
                    Button {
                        appState.switchTo(threadId: thread.id)
                        dismiss()
                    } label: {
                        threadRow(thread)
                    }
                    .buttonStyle(.plain)
                    .listRowBackground(thread.id == appState.currentThreadId
                                       ? Color.accent.opacity(0.15)
                                       : Color.operatorCard)
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            appState.deleteThread(thread.id)
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color.operatorBackground)
            .navigationTitle("Threads")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        appState.createThread()
                        dismiss()
                    } label: {
                        Image(systemName: "plus")
                    }
                }
            }
        }
    }

    private func threadRow(_ thread: ChatThread) -> some View {
        HStack(spacing: Theme.Spacing.regular) {
            Circle()
                .fill(thread.id == appState.currentThreadId ? Color.accent : .operatorSubtle)
                .frame(width: 8, height: 8)

            VStack(alignment: .leading, spacing: 2) {
                Text(thread.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                HStack(spacing: 6) {
                    if let ctx = thread.pinnedContext {
                        Label(String(ctx.sessionId.prefix(8)), systemImage: "pin.fill")
                            .labelStyle(.titleAndIcon)
                            .font(.caption2)
                            .foregroundStyle(.accent)
                    } else {
                        Text("no pinned context")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                    Text("·")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Text(thread.updatedAt, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            Text("\(thread.records.count)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 4)
    }
}
