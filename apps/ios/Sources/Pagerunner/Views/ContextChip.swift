import SwiftUI
import PagerunnerKit

/// Small chip shown above the chat composer that summarises the pinned
/// context for the current thread. Tap action is supplied by the parent.
struct ContextChip: View {
    @Environment(AppState.self) private var appState
    var onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 10) {
                thumbnail
                    .frame(width: 28, height: 28)
                    .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))

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
        .task(id: appState.pinnedContext) {
            if let ctx = appState.pinnedContext, let client = appState.connection.apiClient {
                appState.thumbnails.fetchIfNeeded(ctx, client: client)
            }
        }
    }

    private var thumbnail: some View {
        Group {
            if let ctx = appState.pinnedContext, let img = appState.thumbnails.image(for: ctx) {
                Image(uiImage: img).resizable().scaledToFill()
            } else {
                Image(systemName: appState.pinnedContext == nil ? "questionmark.circle" : "rectangle.dashed")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(.operatorSubtle)
            }
        }
    }

    private var primaryLabel: String {
        guard let ctx = appState.pinnedContext else { return "No context pinned" }
        if let session = appState.sessions.first(where: { $0.id == ctx.sessionId }) {
            return session.profile
        }
        return String(ctx.sessionId.prefix(8)) + "…"
    }

    private var secondaryLabel: String {
        guard let ctx = appState.pinnedContext else { return "Tap to choose" }
        if let session = appState.sessions.first(where: { $0.id == ctx.sessionId }) {
            return session.displayName
        }
        return "session \(String(ctx.sessionId.prefix(8)))"
    }
}
