import SwiftUI
import PagerunnerKit

/// Compact pill above the chat composer. Shows an overlapping stack of up
/// to 3 tab thumbnails, a count badge if more, the thread's Scope goal (or
/// "Scope" fallback), and a secondary line with the tab count.
struct ScopeChip: View {
    @Environment(AppState.self) private var appState
    var onTap: () -> Void

    private var scope: Scope {
        appState.currentThread?.scope ?? Scope()
    }

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 10) {
                thumbnailStack
                    .frame(width: 48, height: 28)

                VStack(alignment: .leading, spacing: 1) {
                    Text(primaryLabel)
                        .font(.footnote.weight(.semibold))
                        .lineLimit(1)
                    Text(secondaryLabel)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .glassEffect(.regular.interactive(), in: .capsule)
        }
        .buttonStyle(.plain)
        .task(id: scope.tabs.map(\.id)) {
            guard let client = appState.connection.apiClient else { return }
            for tab in scope.tabs.prefix(3) {
                let ctx = PinnedContext(sessionId: tab.sessionId, targetId: tab.targetId)
                appState.thumbnails.fetchIfNeeded(ctx, client: client)
            }
        }
    }

    private var thumbnailStack: some View {
        let visible = Array(scope.tabs.prefix(3))
        return ZStack(alignment: .leading) {
            if visible.isEmpty {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(.operatorSubtle)
                    .overlay(
                        Image(systemName: "questionmark.circle")
                            .foregroundStyle(.secondary)
                            .font(.caption2)
                    )
                    .frame(width: 28, height: 28)
            } else {
                ForEach(Array(visible.enumerated()), id: \.element.id) { offset, tab in
                    thumbnail(for: tab)
                        .frame(width: 28, height: 28)
                        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .stroke(Color.operatorBackground, lineWidth: 1.5)
                        )
                        .offset(x: CGFloat(offset) * 10)
                }
            }
            if scope.tabs.count > 3 {
                Text("+\(scope.tabs.count - 3)")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 4)
                    .padding(.vertical, 2)
                    .background(.operatorSubtle, in: Capsule())
                    .offset(x: 40)
            }
        }
    }

    private func thumbnail(for tab: ScopeTab) -> some View {
        let ctx = PinnedContext(sessionId: tab.sessionId, targetId: tab.targetId)
        return Group {
            if let img = appState.thumbnails.image(for: ctx) {
                Image(uiImage: img).resizable().scaledToFill()
            } else {
                Color.operatorSubtle
                    .overlay(Image(systemName: "rectangle.dashed").font(.caption2).foregroundStyle(.tertiary))
            }
        }
    }

    private var primaryLabel: String {
        if let goal = scope.goal, !goal.isEmpty { return goal }
        if scope.tabs.isEmpty { return "No scope pinned" }
        return "Scope"
    }

    private var secondaryLabel: String {
        if scope.tabs.isEmpty { return "Tap to set" }
        if scope.tabs.count == 1 { return scope.tabs[0].label.isEmpty ? "1 tab" : scope.tabs[0].label }
        return "\(scope.tabs.count) tabs"
    }
}
